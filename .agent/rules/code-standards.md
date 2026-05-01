---
trigger: always_on
---

# Code Standards

## Design Philosophy

- **Prefer the most robust, DRY, scalable solution.** Implement comprehensive fixes designed for the long term.
- **Reuse existing patterns.** Check the codebase before building something new.
- **Handle edge cases proactively.** Address race conditions, error states, and boundary cases during initial implementation.

## Rust Conventions

- **Production error handling**: use `?`, `unwrap_or_default()`, or explicit match — **no `.unwrap()`** in production paths.
- **Cloning**: prefer references / `Cow<>` over explicit `.clone()`. Search-filter helpers return `Cow::Borrowed` when no query is active.
- **Logging**: structured `tracing` macros — `error!` (failures), `warn!` (recoverable), `info!` (milestones), `debug!` (flow), `trace!` (per-frame / per-packet / startup enumeration). Stderr is quiet by default; the file log at `~/.local/state/nokkvi/nokkvi.log` stays verbose.
- **Threading**: prefer `Arc` + atomics over `Mutex<T>` for simple shared state. Theme color reads use `ArcSwap` (lock-free).

## Error Handling

- Backend services return `Result<T, E>` — propagate with `?`.
- UI handlers use `shell_task` / `shell_spawn` from `update/components.rs`.
- Log at the boundary that finally handles, not at every propagation layer.
- User-facing errors get `toast_error()` / `toast_warn()`. Confirmations get `toast_success()` / `toast_info()`.

## File Organization

- One view per file, one service per file.
- Complex views/services/handlers use directory modules (e.g., `views/settings/mod.rs`, `services/queue/mod.rs`, `update/hotkeys/mod.rs`).
- Handler files in `update/` correspond 1:1 to views, plus specialized handlers for cross-cutting concerns. `ls src/update/` to see them.
- `shell_task` / `shell_spawn` are methods on `Nokkvi` (`src/main.rs`) — they bridge UI handlers to async `AppService` work.
- Shared helpers in `update/components.rs` — `PaginatedFetch::from_common`, `guard_play_action`, `set_item_rating_task`, `radio_mutation_task`, `handle_common_view_action`, `shell_action_task`, `shell_fire_and_forget_task`, `prefetch_album_artwork_tasks`, `prefetch_song_artwork_tasks`, plus the entity-action helpers (`play_entity_task`, `add_entity_to_queue_task`, `insert_entity_to_queue_at_position_task`, `star_item_task`).

## Core Requirements

- **Audio track changes**: create fresh decoders before locking the engine; release the engine lock during decoder operations.
- **View render output**: keep the root widget type stable across renders (always a `Column`, etc.) — changing it destroys `text_input` focus.
- **Search**: fire queries immediately on text change (no debounce).
- **Visualizer FFT thread**: `try_lock()` only. Only the main render thread may use `lock()`.
- **Play actions**: call `guard_play_action()` at the top of every play handler (split-view + playlist-edit conflict guard).
- **Border radii**: use `ui_border_radius()` from `theme.rs`, not hardcoded values. Iced clips background to border radius even when the border is transparent — leave radius unset on flush-to-edge bars.
- **Single-active overlay menus**: hamburger / kebab / checkbox-dropdown / context menus bubble `Message::SetOpenMenu(...)` to root rather than owning local `is_open` state.

## Dependencies

Discuss before adding new crates. Existing workspace runtime deps:
- **UI crate** (`Cargo.toml`): `iced` (forked), `nokkvi-data`, `tokio`, `tracing` (+ `tracing-subscriber`), `parking_lot`, `arc-swap`, `futures`, `anyhow`, `image`, `notify`, `mpris-server`, `ksni`, `reqwest`, `serde` (+ `serde_json`), `toml` (+ `toml_edit`), `lru`, `bytemuck`, `tempfile`
- **Data crate** (`data/Cargo.toml`): `tokio` (+ `tokio-util`), `parking_lot`, `futures`, `anyhow`, `thiserror`, `image`, `color-thief`, `reqwest`, `serde` (+ `serde_json`), `toml` (+ `toml_edit`), `bincode-next`, `redb`, `chrono`, `directories`, `url`, `httpdate`, `rand`, `font-kit`, `rodio`, `ringbuf`, `rustfft`, `num-complex`, `biquad`, `bytemuck`, `symphonia`, `icy-metadata`, `pipewire` (linux-only)
- **Test-only `[dev-dependencies]`**: `proptest`, `tempfile`

## Formatting

- **All code must pass `cargo +nightly fmt --all`**. Config in `rustfmt.toml` (100-char max, crate-level import merging, std/external/crate import grouping). Nightly is required because of the unstable settings.

## Testing & Verification

### Red-Green TDD Protocol

For bug fixes and new update handlers:

1. **Red**: write tests in `src/update/tests.rs` (or `tests_queue_filter.rs` / `tests_star_rating.rs` — group by area) using the `test_app()` helper. Assert against **observable state mutations** (`modes.random`, `modes.consume`, `search_query`) — never side effects requiring `app_service`. Run, confirm fail.
2. **Green**: minimal implementation to pass.
3. **Verify**: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo +nightly fmt --all`.

If structural plumbing (new fields, message variants) is needed, complete it first so the tests compile, but make no behavioral changes until the tests are red.

**Test placement**: `update/tests*.rs` for handler tests; inline `#[cfg(test)] mod tests` for self-contained logic (data types, widgets, pure functions).

### CI Commands

```bash
cargo +nightly fmt --all                     # format
cargo clippy --all-targets -- -D warnings    # lint, zero-warning gate
cargo test                                   # all tests
cargo build --release                        # release build
```

## Config & Persistence

| Store | What | How |
|-------|------|-----|
| `config.toml` | User preferences (general, interface, playback, hotkeys, views, visualizer behavior, font, artwork resolution, library page size, normalization, tray, etc.) | Hot-reloadable via `SettingsManager` + `config_writer.rs`. `verbose_config` writes all defaults |
| Theme files | Named `.toml` in `~/.config/nokkvi/themes/` | Palette + visualizer colors. **21 built-in**. `config.toml` stores `theme = "name"` |
| redb | Queue, session tokens (JWT, Subsonic), encrypted password | Via `state_storage.rs`, `services/queue/`, `credentials.rs` |
| Credentials | Server URL, username | In `config.toml`. Password is **not** stored on disk in plaintext |

Settings actions carry a typed `ConfigKey` (`AppScalar` / `AppArrayEntry` / `Theme` / `ThemeArrayEntry`) so the writer matches on the variant rather than sniffing key prefixes.

## Rule Maintenance

When a commit changes **architecture, patterns, or conventions**, check `.agent/rules/` for staleness. Flag stale rules before committing.
