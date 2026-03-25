---
trigger: always_on
---

# Code Standards

## Design Philosophy

- **Always prefer the most robust, DRY, and scalable solution.** No quick fixes, no half-measures.
- **Reuse existing patterns.** Check the codebase before building something new.
- **Handle edge cases proactively.** Address race conditions, error states, and boundary cases during initial implementation.

## Rust Conventions

- **No `.unwrap()` in production code.** Use `?`, `.unwrap_or_default()`, or explicit error handling.
- **No `clone()` without justification.** Prefer references or `Cow<>`.
- **Structured logging** via `tracing`: `error!` (failures), `warn!` (recoverable), `info!` (milestones), `debug!` (flow control), `trace!` (per-frame, per-packet, startup enumeration). Audio modules and queue service use `trace!` for high-frequency operations.
- **Use `Arc` + atomics** for cross-thread shared state, never raw `Mutex` around simple values.

## Error Handling

- Backend services return `Result<T, E>` — propagate with `?`.
- UI handlers use `shell_action_task` / `shell_fire_and_forget_task` helpers.
- Log errors at the boundary where they're handled, not where they're propagated.
- **Toast on user-facing errors**: `toast_error()` / `toast_warn()`. Use `toast_success()` / `toast_info()` for confirmations.

## File Organization

- Keep files focused: one view per file, one service per file.
- Complex views use directory modules (`views/settings/mod.rs`).
- Complex services use directory modules (`services/queue/mod.rs` + `order.rs`, `navigation.rs`).
- Complex handlers use directory modules (`update/hotkeys/mod.rs` + `star_rating.rs`, `queue.rs`, `navigation.rs`).
- Handler files in `update/` correspond 1:1 to views, plus specialized handlers:
  - `browsing_panel.rs` — split-view playlist editing mode management
  - `cross_pane_drag.rs` — drag state machine (browsing panel → queue)
  - `toast.rs` — notification dispatch
  - `slot_list.rs` — shared slot list navigation dispatch + scrollbar fade/seek-settled timers (view-targeted via explicit `View` parameter)
  - `SlotListMessage` — Navigate up/down, set offset, activate center, toggle sort, scrollbar timers (carry `View`)
  - `collage.rs` — genre/playlist artwork collage loading
  - `hotkeys/` — directory module: `mod.rs` (core dispatch), `star_rating.rs`, `queue.rs`, `navigation.rs`
  - `navigation.rs` — view switching, browsing panel toggle
  - `playback.rs` — playback tick, transport, gapless transitions
  - `player_bar.rs` — player bar action dispatch (transport, volume, visualization)
  - `scrobbling.rs` — scrobble submission and now-playing notifications
  - `mpris.rs` — MPRIS D-Bus event handling
  - `window.rs` — window resize handling and centralized artwork prefetch dispatch
  - `settings.rs` — settings action dispatch (config writes, general settings, hotkeys, presets, cache rebuild, logout)
  - `progressive_queue.rs` — progressive queue page append chain
  - `info_modal.rs` — info modal open/close dispatch
  - `text_input_dialog.rs` — text input dialog open/submit/cancel dispatch
  - `components.rs` — shared helpers (`shell_action_task`, `guard_play_action`, `set_item_rating_task`, etc.)

## Anti-Patterns — Do NOT

- **Hold the audio engine lock during decoder operations.** Create fresh decoders on track change.
- **Change the root widget type between renders** (e.g., Row→Column). Destroys `text_input` focus.
- **Install unnecessary dependencies.** Core deps: `reqwest`, `serde`, `bincode`, `redb`, `toml_edit`, `font-kit`, `lru`, `rodio`, `ringbuf`, `bytemuck`, `rustfft`. Don't add alternatives.
- **Use debounce on search.** Fires immediately on query change.
- **Use `lock()` in the visualizer FFT processing thread.** FFT thread uses `try_lock()`; render thread uses `lock()`.
- **Allow play actions from the browsing panel.** Use `guard_play_action()`.

## Formatting

- **All code must pass `cargo +nightly fmt --all`**. Config in `rustfmt.toml` (100-char max, crate-level import merging, std/external/crate import grouping).

## Testing & Verification

```bash
cargo +nightly fmt --all      # Format (nightly required)
cargo clippy                  # Lint — fix all warnings
cargo test                    # Unit tests
cargo build --release         # Release build verification
```

Tests live in inline `#[cfg(test)]` modules. Key test locations: `update/tests.rs`, `data/src/services/queue/mod.rs`, `data/src/services/queue/navigation.rs`, `data/src/types/hotkey_config.rs`, `data/src/types/paged_buffer.rs`, `data/src/types/player_settings.rs`, `data/src/credentials.rs`, `data/src/audio/spectrum.rs`, `src/embedded_svg.rs`, `src/widgets/format_info.rs`, `src/views/settings/items.rs` (general + interface + playback structure tests). Additional `#[cfg(test)]` modules exist in various type/utility files (`data/src/types/song_pool.rs`, `data/src/utils/`, `src/widgets/slot_list*.rs`, `src/views/expansion.rs`, `src/update/mod.rs`).

## Config & Persistence

| Store | What | How |
|-------|------|-----|
| `config.toml` | Theme + visualizer settings | Hot-reloadable, writes via `config_writer.rs` (preserves comments, auto-injects descriptions). No secrets. |
| redb | Queue, player settings, hotkey bindings, encrypted password | Via `state_storage.rs`, `queue/`, `SettingsManager`. |
| Credentials | Server URL, username, password | AES-256-GCM encrypted, password in redb |

## Rule Maintenance

When a commit changes **architecture, patterns, or conventions**, check if `.agent/rules/` files need updating. Flag stale rules before committing.
