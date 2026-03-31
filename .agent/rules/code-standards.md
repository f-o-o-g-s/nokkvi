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
- **Structured logging** via `tracing`: `error!` (failures), `warn!` (recoverable), `info!` (milestones), `debug!` (flow control), `trace!` (per-frame, per-packet, startup enumeration).
- **Use `Arc` + atomics** for cross-thread shared state, never raw `Mutex` around simple values.

## Error Handling

- Backend services return `Result<T, E>` — propagate with `?`.
- UI handlers use `shell_action_task` / `shell_fire_and_forget_task` helpers.
- Log errors at the boundary where they're handled, not where they're propagated.
- **Toast on user-facing errors**: `toast_error()` / `toast_warn()`. Use `toast_success()` / `toast_info()` for confirmations.

## File Organization

- Keep files focused: one view per file, one service per file.
- Complex views/services/handlers use directory modules (e.g., `views/settings/mod.rs`, `services/queue/mod.rs`, `update/hotkeys/mod.rs`).
- Handler files in `update/` correspond 1:1 to views, plus specialized handlers for cross-cutting concerns. `ls src/update/` to see them.
- Shared helpers live in `update/components.rs`.

## Anti-Patterns — Do NOT

- **Hold the audio engine lock during decoder operations.** Create fresh decoders on track change.
- **Change the root widget type between renders** (e.g., Row→Column). Destroys `text_input` focus.
- **Install unnecessary dependencies.** Core deps: `reqwest`, `serde`, `bincode`, `redb`, `toml_edit`, `font-kit`, `lru`, `rodio`, `ringbuf`, `bytemuck`, `rustfft`, `pipewire`. Don't add alternatives.
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

Tests live in inline `#[cfg(test)]` modules. Grep for `#[cfg(test)]` to find them.

## Config & Persistence

| Store | What | How |
|-------|------|-----|
| `config.toml` | User preferences (General, Interface, Playback, Hotkeys, Views, Visualizer behavior) | Hot-reloadable via `SettingsManager` & `config_writer.rs`. `verbose_config` mode ensures defaults are output. |
| Theme files | Named `.toml` files in `~/.config/nokkvi/themes/` | Palette colors, visualizer colors, font family. 11 built-in themes. `config.toml` stores `theme = "name"` key. |
| redb | Queue, encrypted password | Via `state_storage.rs`, `queue/`. |
| Credentials | Server URL, username, password | AES-256-GCM encrypted, password in redb |

## Rule Maintenance

When a commit changes **architecture, patterns, or conventions**, check if `.agent/rules/` files need updating. Flag stale rules before committing.
