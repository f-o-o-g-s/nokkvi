---
trigger: always_on
---

# Project Context — Nokkvi

A Rust/Iced desktop client for [Navidrome](https://www.navidrome.org/) music servers.

## Crate Structure

| Crate | Path | Purpose |
|-------|------|---------|
| **UI** | `src/` | Iced frontend: views, widgets, update handlers, subscriptions, theme |
| **Data** | `data/` | Iced-free backend: domain types, services, audio engine, API client, persistence |

**Entry points:** `src/main.rs`, `src/app_message.rs` (root Message enum), `src/update/mod.rs` (central dispatcher), `data/src/backend/app_service.rs` (backend orchestrator).

**Key data structure:** `PagedBuffer<T>` (`data/src/types/paged_buffer.rs`) — replaces `Vec<T>` for all library data. `Deref<Target = [T]>` makes it a drop-in replacement. Load state tracked via `set_loading()` / `needs_fetch()`.

**Consolidated state:** `src/state.rs` groups app state into domain structs (`PlaybackState`, `ActivePlayback` (Queue/Radio), `ScrobbleState`, `LibraryData`, `WindowState`, `ToastState`, `SimilarSongsState`, etc.).

## Core Pattern: TEA (The Elm Architecture)

Every view follows this structure — do not deviate:

```rust
// 1. State struct
pub struct AlbumsPage { common: SlotListPageState, ... }

// 2. Local message enum
pub enum AlbumsMessage { SlotListNavigateUp, SlotListNavigateDown, ... }

// 3. Action enum (bubbles to root for side effects)
pub enum AlbumsAction { PlayAlbum(String), None }

// 4. update() returns (Task, Action)
fn update(&mut self, msg: AlbumsMessage) -> (Task<AlbumsMessage>, AlbumsAction)

// 5. view() is pure, receives borrowed ViewData from app state
fn view<'a>(&'a self, data: AlbumsViewData<'a>) -> Element<'a, AlbumsMessage>
```

**ViewData structs borrow app state** (`&'a` references, not clones). The `large_artwork` field borrows a pre-computed `HashMap` snapshot refreshed after each LRU mutation.

**Shared traits:** `ViewPage` trait (explicit `impl` per view, no macro). `CommonViewAction` + `HasCommonAction` trait for generic SearchChanged/SortModeChanged/SortOrderChanged handling. `impl_expansion_update!` macro for expansion view deduplication.

**Root routing** in `update/mod.rs` dispatches `Message::Albums(msg)` to the page, then handles the returned Action.

## Message Architecture

The root `Message` enum uses **namespaced sub-enums**: `PlaybackMessage`, `ScrobbleMessage`, `HotkeyMessage`, `ArtworkMessage`, `SlotListMessage` (carries `View`), `ToastMessage`. Flat variants remain for cross-cutting concerns. See `src/app_message.rs`.

## Naming Conventions

| Convention | Example |
|------------|---------|
| View pages | `{Name}Page`, `{Name}Message`, `{Name}Action`, `{Name}ViewData` |
| Update handlers | `update/{name}.rs` |
| Backend services | `data/src/backend/{name}.rs` |
| API endpoints | `data/src/services/api/{name}.rs` |
| Domain types | `data/src/types/{name}.rs` |
| Slot list widgets | `widgets/slot_list.rs` (rendering via `SlotListRowMetrics`), `widgets/slot_list_view.rs` (scroll state), `widgets/slot_list_page.rs` (page state) |

## Directories to Skip

`target/`, `dist/`, `docs/`, `scripts/` — not project code.

## Reference Codebases

External repos cloned locally for reference (not part of this project). Browse `reference-{name}/` when needed:

`iced`, `iced-apps`, `symphonia`, `feishin`, `lucide` (icons), `navidrome`, `pipewire`, `pipewire-rs`, `rmpc`, `ferrosonic`, `saxon`, `wireplumber`, `mpd`, `rodio`, `rustfft`, `cava`, `bincode`, `toml`, `musikcube`
