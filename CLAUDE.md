# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Nokkvi is a native Rust/Iced desktop client for Navidrome music servers. **Linux-only** (built/tested on Arch + Wayland/Hyprland with PipeWire). The codebase is AI-generated and AI-maintained — the human owner directs and tests but does not write code.

## Common commands

```bash
cargo build                                  # debug build
cargo build --release                        # release build → target/release/nokkvi
cargo test                                   # all tests (workspace)
cargo test -p nokkvi <name_substring>        # single test in UI crate
cargo test -p nokkvi-data <name_substring>   # single test in data crate
cargo test --bin nokkvi -- embedded_svg      # icon-registration test (silently fails otherwise — see "Gotchas")
cargo clippy --all-targets -- -D warnings    # lint with CI strictness (zero warnings allowed)
cargo +nightly fmt --all                     # format (NIGHTLY rustfmt required — see rustfmt.toml)
cargo +nightly fmt --all -- --check          # format check
```

CI runs all four checks (fmt-check / clippy `-D warnings` / test / release build). All must pass before merging. `rustfmt.toml` uses unstable features (`imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`), which is why nightly is required.

System dependencies (Arch): `pacman -S pipewire fontconfig pkg-config`. The audio engine links against `libpipewire-0.3` at build time.

Per-user data lives in `~/.config/nokkvi/` (config.toml, app.redb, themes/, cache/, sfx/, nokkvi.log). The log file is truncated on every launch.

## Workspace layout

Two crates:

| Crate | Path | Role |
|-------|------|------|
| `nokkvi` | `src/` | Iced UI: views, widgets, update handlers, subscriptions, theme. Depends on `nokkvi-data`. |
| `nokkvi-data` | `data/` | **Iced-free** backend: domain types, audio engine, Subsonic/Navidrome API client, persistence, services. |

Entry points: `src/main.rs` (Iced app + `Nokkvi` root state), `src/app_message.rs` (root `Message` enum), `src/update/mod.rs` (central dispatcher), `data/src/backend/app_service.rs` (`AppService` backend orchestrator).

`reference-*/` directories are external repos cloned for reference (iced, symphonia, feishin, rmpc, navidrome, lucide icons, pipewire, rodio, etc.). They are **not** project code — do not edit them, but read freely. `target/`, `dist/`, `docs/`, `.venv/`, `tmp/` are also non-project.

## Architecture: The Elm Architecture (TEA)

Every view follows this pattern — do not deviate:

```rust
pub struct AlbumsPage { common: SlotListPageState, /* ... */ }
pub enum AlbumsMessage { SlotListNavigateUp, /* ... */ }
pub enum AlbumsAction { PlayAlbum(String), None }
fn update(&mut self, msg: AlbumsMessage) -> (Task<AlbumsMessage>, AlbumsAction);
fn view<'a>(&'a self, data: AlbumsViewData<'a>) -> Element<'a, AlbumsMessage>;  // pure
```

`view()` is pure and receives a `{Name}ViewData` struct that **borrows** app state (`&'a` references, not clones). The root `Nokkvi::update` dispatches `Message::Albums(msg)` to the page, then handles the returned `AlbumsAction` for side effects (toasts, AppService calls, navigation).

Key shared infrastructure:
- `ViewPage` trait (`views/mod.rs`) — explicit `impl` per view, no macro. Has pane-aware `current_view_page{,_mut}()` (delegates to browsing panel in split-view) and direct `view_page{,_mut}(View)`.
- `CommonViewAction` + `HasCommonAction` — generic SearchChanged/SortModeChanged/SortOrderChanged dispatch. Handled centrally by `handle_common_view_action()` in `update/components.rs`.
- `impl_expansion_update!` macro — deduplicates inline expansion handling.
- `SlotListPageState` — shared state for every slot-list view (search, scroll, focus, multi-selection set).
- Helpers in `update/components.rs`: `shell_task` / `shell_spawn` (run async work against `AppService`), `guard_play_action` (block plays during playlist edit / split-view conflicts), `set_item_rating_task`, `radio_mutation_task`.

Root `Message` is namespaced via sub-enums (`PlaybackMessage`, `ScrobbleMessage`, `HotkeyMessage`, `ArtworkMessage`, `SlotListMessage` (carries `View`), `ToastMessage`). Flat variants remain only for cross-cutting concerns. See `src/app_message.rs`.

## Backend (`data/`) architecture

```
AppService (orchestrator)
├── PlaybackController       — audio engine + queue navigator + transport + history + reset_next_track()
├── Domain Services          — Albums, Artists, Songs, Genres, Playlists, Radios, Similar, Queue,
│                              Settings, Auth (lazy via tokio OnceCell)
├── ArtworkPrefetch          — background library-wide artwork download w/ pagination + dynamic key map
├── NavidromeEvents          — SSE subscription → triggers ID-anchored library refresh
└── TaskManager              — centralized spawn tracking + status channel for UI notifications
```

- **`PagedBuffer<T>`** (`data/src/types/paged_buffer.rs`) replaces `Vec<T>` for all library data. `Deref<Target = [T]>` makes it drop-in. Load state via `set_loading()` / `needs_fetch()`. Always call `set_loading(true)` before dispatching a page fetch — otherwise rapid scroll triggers duplicate fetches.
- **Persistence**: `redb` (`app.redb`) for queue/session/structured state via `services/state_storage.rs`; TOML (`config.toml`) for user-editable config via `services/toml_settings_io.rs` and `src/config_writer.rs`. **Routing matters**: `update_config_value()` writes `config.toml`; `update_theme_value()` writes the active theme file in `~/.config/nokkvi/themes/`. Misrouting silently overwrites the wrong file.
- **Queue serialization** is bincode (`Encode`/`Decode`); `load_binary_or_json()` migrates legacy JSON.
- **Domain types are iced-free.** Anything in `data/src/types/` must not import `iced`.

## Audio engine (`data/src/audio/`)

Native PipeWire output via a shared `rodio::Mixer`:

```
CustomAudioEngine
├── AudioDecoder (Symphonia) — Standard: HTTP w/ RangeHttpReader (256KB chunks, 16-chunk LRU, prefetch)
│                              Radio: AsyncNetworkBuffer (tokio→bounded mpsc→sync Read) + auto-reconnect
├── AudioRenderer (ring buffers) → visualizer callback from StreamingSource
│   └── RodioOutput (shared Mixer) → ActiveStream per track
│       └── StreamingSource (rodio::Source) → EqProcessor → lock-free ring buffer → pipewire callback
├── CrossfadePhase: Idle → Active → OutgoingFinished
└── EqState — shared atomic gains, biquad filter bank per stream
```

Critical invariants:
- **Track changes**: create fresh decoders **before** locking the engine; release the engine lock during decoder operations. Never hold the lock across decoder creation.
- **Visualizer FFT thread uses `try_lock()` only**; only the main render thread may use `lock()`.
- **`source_generation: AtomicU64`** — engine increments on `set_source()`, renderer snapshots and discards stale callbacks. This prevents consume+shuffle from replaying the just-consumed track.
- **Crossfade trigger must be synchronous**: set `crossfade_active = true` in the same tick as the position check, then signal the engine async. Otherwise EOF fires first → hard cut.
- **Mode toggles** (shuffle/repeat/consume) must call `reset_next_track()` to clear the prepared decoder and disarm crossfade.
- **Visualizer samples are pre-volume**, scaled to S16 range — FFT is volume-independent.

## Conventions and required rules

- **Errors**: production code uses `?`, `unwrap_or_default()`, or explicit match — **no `.unwrap()`** in production paths. Backend services return `Result<T, E>`. Log at the boundary that finally handles, not at every propagation layer. User-facing errors get `toast_error()` / `toast_warn()`.
- **Logging**: structured `tracing` macros — `error!` (failures), `warn!` (recoverable), `info!` (milestones), `debug!` (flow), `trace!` (per-frame/per-packet/startup enumeration).
- **Cloning**: prefer references / `Cow<>` over `.clone()`. Search filter helpers return `Cow::Borrowed` when no query is active (zero-cost).
- **Threading**: prefer `Arc` + atomics over `Mutex<T>` for simple shared state.
- **Search**: always immediate — never debounce.
- **Dependencies**: rely on the existing workspace crates; discuss before adding new ones. Runtime: `iced`, `tokio`, `tracing` (+ `tracing-subscriber`), `parking_lot`, `futures`, `anyhow`, `image`, `notify`, `mpris-server`, `reqwest`, `serde` (+ `serde_json`), `toml` (+ `toml_edit`), `bincode-next`, `redb`, `chrono`, `directories`, `url`, `httpdate`, `rand`, `lru`, `bytemuck`, `font-kit`, `rodio`, `ringbuf`, `rustfft`, `num-complex`, `biquad`, `symphonia`, `icy-metadata`, `color-thief`, `thiserror`, `pipewire` (linux-only). Test-only `[dev-dependencies]`: `proptest`, `tempfile`.
- **Render output**: keep a view's root widget type stable across renders (e.g., always `Column`) — changing it destroys `text_input` focus. Use `base_slot_list_empty_state` for empty/loaded parity.
- **Border radii**: use `ui_border_radius()` (theme-aware via `ROUNDED_MODE` atomic), not hardcoded values. Iced clips background to border radius even when the border is transparent — leave radius unset on flush-to-edge bars.
- **Manual UI verification (overrides default Claude Code guidance)**: nokkvi is a native Rust/Iced desktop app — there is no browser, no dev server, no `npm run dev`. Ignore any default instruction to "start the dev server" or "test in a browser". When the human owner asks for a UI change, deliver code that compiles cleanly (`cargo build`), passes tests/clippy/fmt, and stop there. The human runs `cargo run` (or a release build) and tests the running window themselves; their feedback is the verification loop. If a change has UI implications you cannot validate from code alone (visual layout, focus, marquee timing, etc.), say so explicitly in the handoff so the owner knows what to look at.

## Red-Green TDD for handlers

When fixing a bug or adding a new update handler:

1. **Red** — write tests in `src/update/tests.rs` using the `test_app()` helper. Assert against **observable state mutations** (e.g., `modes.random`, `modes.consume`, `search_query`) — never side effects requiring `app_service`. Run, confirm fail.
2. **Green** — minimal implementation to pass.
3. **Verify** — `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo +nightly fmt --all`.

If structural plumbing (new fields, message variants) is needed, complete it first so the tests compile, but make no behavioral changes until the tests are red.

Test placement: `update/tests.rs` for handler tests; inline `#[cfg(test)] mod tests` for self-contained logic (data types, widgets, pure functions).

## Gotchas (the silent ones)

- **Embedded SVG icons fall back silently to play.svg.** Adding an icon means: copy SVG to `assets/icons/`, add `const` + `include_str!` in `src/embedded_svg.rs`, add a match arm in `get_svg()`, **and** add to the `KNOWN` test array. Compiler will not warn if you forget. Run `cargo test --bin nokkvi -- embedded_svg` to catch unbound paths.
- **Artwork**: use `iced::widget::image::Handle::from_bytes(data)` for refreshable artwork — `Handle::from_path` keys on path and produces stale GPU textures when the file is overwritten. After every `put()` / `get()` on the artwork LRU, call `refresh_large_artwork_snapshot()` so `ViewData.large_artwork` borrows the new map.
- **Queue artwork URLs**: queue song mini thumbnails MUST request 80px using `album_id` to hit the prefetch cache; large artwork fallback MUST construct the full-size URL (`size=1000`) — never reuse the 80px URL.
- **Filtered queue indices**: when a search is active, slot-list indices are relative to `filtered_songs`. Always map through the filtered view before doing queue mutations.
- **Queue navigation**: use `peek_next_song()` → `transition_to_queued()` for transitions. Use `set_current_index()` ONLY for non-transition updates like play-from-here.
- **`HoverOverlay` wraps containers, never native buttons** — buttons capture `ButtonPressed` early. Pattern: `mouse_area(HoverOverlay::new(container(...))).on_press(msg)`.
- **`guard_play_action()` at the top of every play handler** — protects against split-view + playlist-edit conflicts.
- **Config-watcher feedback loops**: `suppress_config_reload()` blocks the file watcher's reflection, but GUI-initiated theme/visualizer writes need a manual `ThemeConfigReloaded` trigger after the write.
- **Database lock on re-login**: `StateStorage` is cached on `Nokkvi.cached_storage` and reused via `AppService::new_with_storage()` — redb holds an exclusive lock so a fresh open after logout will fail. Stop the engine + `TaskManager` on logout.
- **`CenterOnPlaying` (Shift+C)**: call `handle_set_offset()` directly. Dispatching `SlotListMessage::SetOffset` routes through the click-to-highlight path instead.

For the full set of rules and patterns, see `.agent/rules/` (loaded contextually):
- `project-context.md` — crate structure, naming, key types
- `code-standards.md` — formatting, error handling, TDD protocol
- `audio-engine.md` — engine internals, crossfade, EQ
- `backend-services.md` — services, persistence, queue system
- `ui-views.md` — slot list views, expansion, browsing panel, modals
- `widgets.md` — widget catalog, layout constants, 3D buttons, SVG icons
- `visualizer.md` — FFT pipeline, shaders, peak/gradient modes
- `settings-view.md` — settings module structure, drill-down, SettingValue types
- `gotchas.md` — full list of the subtle pitfalls
- `new-feature-checklist.md` — end-to-end checklist for new features

Workflows in `.agent/workflows/` (`build-test.md`, `commit.md`, `new-view.md`, `package.md`, `sync-rules.md`) document concrete procedures.

## Commit conventions

Conventional Commits: `type(scope): description` (lowercase, imperative, no trailing period). Types: `feat`, `fix`, `refactor`, `perf`, `style`, `chore`, `docs`, `test`, `ci`. Common scopes: `audio`, `queue`, `ui`, `api`, `settings`, `theme`, `visualizer`, `playback`, `scrobble`, `widgets`, `views`, `hotkeys`, `mpris`, `artwork`, `deps`. Breaking changes use `type(scope)!: ...`. The `.githooks/pre-commit` script auto-updates the Navidrome/PipeWire version pins in `README.md`.
