---
trigger: always_on
---

# Project Context — Nokkvi

A Rust/Iced desktop client for [Navidrome](https://www.navidrome.org/) music servers. Linux-only.

## Crate Structure

| Crate | Path | Purpose |
|-------|------|---------|
| **UI** | `src/` | Iced frontend: views, widgets, update handlers, subscriptions, theme |
| **Data** | `data/` | Iced-free backend: domain types, services, audio engine, API client, persistence |

**Entry points:** `src/main.rs`, `src/app_message.rs` (root `Message` enum + `OpenMenu` enum), `src/update/mod.rs` (central dispatcher), `data/src/backend/app_service.rs` (backend orchestrator).

**Key data structure:** `PagedBuffer<T>` (`data/src/types/paged_buffer.rs`) replaces `Vec<T>` for library data. `Deref<Target=[T]>` makes it a drop-in. Tracks load state via `set_loading()` / `needs_fetch()`. Exposes a monotonic `generation()` counter that bumps on every mutation — pair with `(query, generation)` keys when memoizing.

**Consolidated state** in `src/state/`: per-domain submodules (`panes`, `roulette`, `pending`, `session`, `playback`, `scrobble`, `audio`, `artwork`, `window`, `library`, `toast`, `similar`) re-exported via `state/mod.rs` so call sites use `crate::state::Foo`. Plus `Nokkvi.open_menu: Option<OpenMenu>` as the single-active overlay-menu coordinator (Hamburger / PlayerModes / CheckboxDropdown { view, trigger_bounds } / CheckboxDropdownSimilar { trigger_bounds } / Context { id: ContextMenuId, position }) and `Nokkvi.default_playlist_picker` for the modal picker overlay shared between Playlists and Queue views.

## Core Pattern: TEA (The Elm Architecture)

Every view follows this — do not deviate:

```rust
pub struct AlbumsPage { common: SlotListPageState, /* ... */ }
pub enum AlbumsMessage { SlotListNavigateUp, /* ... */ }
pub enum AlbumsAction { PlayAlbum(String), None }
fn update(&mut self, msg: AlbumsMessage) -> (Task<AlbumsMessage>, AlbumsAction);
fn view<'a>(&'a self, data: AlbumsViewData<'a>) -> Element<'a, AlbumsMessage>;  // pure
```

**ViewData borrows app state** (`&'a` references, not clones). The artwork fields borrow pre-computed `HashMap` snapshots refreshed after every LRU mutation.

**Shared infrastructure:**
- `ViewPage` trait (`views/mod.rs`) — explicit `impl` per view, no macro. Hotkey dispatch + pane-aware routing.
- `CommonViewAction` + `HasCommonAction` — generic SearchChanged / SortModeChanged / SortOrderChanged dispatch.
- `impl_expansion_update!` macro — deduplicates inline expansion handling.
- `SlotListPageState` — shared state for every slot-list view (search, scroll, focus, multi-selection set).
- `PaginatedFetch::from_common()` (`update/components.rs`) — needs_fetch-gated paginated load helper used by Albums / Artists / Songs.

Root routing in `update/mod.rs` dispatches `Message::Albums(msg)` → `albums_page.update(msg)`, then handles the returned Action.

## Message Architecture

Root `Message` is namespaced: `PlaybackMessage`, `ScrobbleMessage`, `HotkeyMessage`, `ArtworkMessage`, `SlotListMessage` (carries `View`), `ToastMessage`. Cross-cutting variants stay flat. See `src/app_message.rs`.

## Pages on `Nokkvi`

`login_page`, `albums_page`, `artists_page`, `genres_page`, `playlists_page`, `queue_page`, `songs_page`, `radios_page`, `settings_page`, `similar_page`. The browsing panel (`views/browsing_panel.rs`) reuses `AlbumsPage` / `SongsPage` / `ArtistsPage` / `GenresPage` / `SimilarPage` via `BrowsingView`.

## Naming Conventions

| Convention | Example |
|------------|---------|
| View pages | `{Name}Page`, `{Name}Message`, `{Name}Action`, `{Name}ViewData` |
| Update handlers | `update/{name}.rs` |
| Backend services | `data/src/backend/{name}.rs` |
| API endpoints | `data/src/services/api/{name}.rs` |
| Domain types | `data/src/types/{name}.rs` |
| Slot list widgets | `widgets/slot_list.rs` (rendering + `SlotListRowMetrics`), `slot_list_view.rs` (scroll), `slot_list_page.rs` (page state) |

## Directories to Skip

`target/`, `dist/`, `tmp/`, `local/`, `.venv/` — not project code.

## Reference Codebases

External repos cloned locally for read-only reference (not part of this project). `reference-{name}/`: `iced`, `iced-apps`, `iced-book`, `iced-docs`, `symphonia`, `feishin`, `lucide` (icons), `navidrome`, `rmpc`, `rmpc-docs`, `rodio`.
