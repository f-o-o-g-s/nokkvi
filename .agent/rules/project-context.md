---
trigger: always_on
---

# Project Context — Nokkvi

A Rust/Iced desktop client for [Navidrome](https://www.navidrome.org/) music servers. Named after Old Norse *nökkvi* (a small, humble boat).

## Crate Structure

| Crate | Path | Purpose |
|-------|------|---------|
| **UI** | `src/` | Iced frontend: views, widgets, update handlers, subscriptions, theme |
| **Data** | `data/` | Iced-free backend: domain types, services, audio engine, API client, persistence |

**Entry points:** `src/main.rs` (Nokkvi), `src/app_message.rs` (root Message enum), `src/update/mod.rs` (central dispatcher), `data/src/backend/app_service.rs` (backend orchestrator).

**Key data structure:** `PagedBuffer<T>` (`data/src/types/paged_buffer.rs`) — replaces `Vec<T>` for all library data. `Deref<Target = [T]>` makes it a drop-in replacement. Load state tracked via `set_loading()` / `needs_fetch()`.

**Consolidated state:** `src/state.rs` groups app state into domain structs (`PlaybackState`, `ScrobbleState`, `LibraryData`, `WindowState`, `ToastState`, etc.).

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

The root `Message` enum uses **namespaced sub-enums**:

| Sub-Enum | Domain |
|----------|--------|
| `PlaybackMessage` | Playback control, ticks, audio render |
| `ScrobbleMessage` | Now playing, submission |
| `HotkeyMessage` | Keyboard actions, star/rating updates |
| `ArtworkMessage` | Artwork pipeline, prefetch, collages |
| `SlotListMessage` | Navigate up/down, set offset, activate center, toggle sort, scrollbar timers (carry `View`) |
| `ToastMessage` | Push / dismiss / dismiss-by-key |

`Message::StripClicked` — centralized left-click handler for the track info strip. Reads `strip_click_action` theme atomic and dispatches: navigate to queue/album/artist, copy track info to clipboard, or do nothing.

`Message::StripContextAction(StripContextEntry)` — right-click context menu handler for the strip. Actions: GoToQueue, GoToAlbum, GoToArtist, CopyTrackInfo, ToggleStar, ShowInFolder.

Flat `Message` variants remain for cross-cutting concerns. See `src/app_message.rs`.

## Naming Conventions

| Convention | Example |
|------------|---------|
| View pages | `{Name}Page`, `{Name}Message`, `{Name}Action`, `{Name}ViewData` |
| Update handlers | `update/{name}.rs` |
| Backend services | `data/src/backend/{name}.rs` |
| API endpoints | `data/src/services/api/{name}.rs` |
| Domain types | `data/src/types/{name}.rs` |
| Slot list widgets | `widgets/slot_list.rs` (rendering), `widgets/slot_list_view.rs` (scroll state), `widgets/slot_list_page.rs` (page state) |

## Directories to Skip (not project code)

- `target/` — Build artifacts
- `dist/` — Distribution packages
- `docs/` — Documentation (inspirations, design notes)
- `example_themes/` — Example TOML theme files (read-only reference)
- `scripts/` — Theme showcase scripts

## Reference Codebases (browse when needed)

These are external repos cloned locally — **not** part of this project:

| Directory | What | When to browse |
|-----------|------|----------------|
| `reference-iced/` | Iced GUI framework source + examples | Widget APIs, layout, subscription patterns, custom shader widgets |
| `reference-iced-apps/` | Community Iced applications | Real-world patterns, complex widget composition |
| `reference-symphonia/` | Symphonia audio decoding library source | Codec APIs, packet/frame handling, format probing |
| `reference-feishin/` | Feishin (TypeScript Navidrome client) | Subsonic/Navidrome API usage patterns, feature ideas |
| `reference-lucide/` | Lucide icon library (1500+ SVGs) | Browse `icons/` dir for new SVG icons to embed |
| `reference-navidrome/` | Navidrome server source (Go) | Server-side API behavior, database schema |
| `reference-pipewire/` | PipeWire Rust bindings source | PipeWire API, stream setup, spa pod construction |
| `reference-pipewire-rs/` | PipeWire Rust crate source | PipeWire Rust API patterns |
| `reference-rmpc/` | rmpc (Rust MPD client, Iced-based) | Iced patterns, MPD protocol, TUI/GUI hybrid |
| `reference-ferrosonic/` | Ferrosonic (Rust Subsonic client) | Subsonic API patterns in Rust |
| `reference-saxon/` | Saxon (Rust audio player) | Audio playback patterns, player architecture |
| `reference-wireplumber/` | WirePlumber Rust bindings | PipeWire session management, policy scripting |
| `reference-mpd/` | MPD (Music Player Daemon) C source | Crossfade behavior, consume/shuffle logic, queue management |
| `reference-rodio/` | rodio audio playback library | Mixer API, source chaining, volume control |
| `reference-rustfft/` | RustFFT library source | FFT plan creation, threading, performance |
| `reference-cava/` | CAVA audio visualizer (C) | Spectrum analysis, smoothing algorithms, bar rendering |
| `reference-bincode/` | bincode serialization library | Encode/Decode traits, binary format |
| `reference-toml/` | toml/toml_edit crate source | TOML parsing, preserving comments on edit |
| `reference-musikcube/` | musikcube (C++ music player) | Player architecture, audio engine patterns |

## Key References

- `.agent/rules/` — Domain-specific rules (auto-attached by file glob)
