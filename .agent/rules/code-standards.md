---
trigger: always_on
---

# Code Standards

## Design Philosophy

- **Always prefer the most robust, DRY, and scalable solution.** ALWAYS implement comprehensive fixes designed for the long term.
- **Reuse existing patterns.** Check the codebase before building something new.
- **Handle edge cases proactively.** Address race conditions, error states, and boundary cases during initial implementation.

## Rust Conventions

- **WHEN handling errors in production code, ALWAYS** use `?`, `.unwrap_or_default()`, or explicit match statements.
- **WHEN passing values, ALWAYS** prefer references or `Cow<>` over explicit `.clone()`.
- **WHEN logging, ALWAYS** use structured logging via `tracing`: `error!` (failures), `warn!` (recoverable), `info!` (milestones), `debug!` (flow control), `trace!` (per-frame, per-packet, startup enumeration).
- **WHEN sharing simple state across threads, ALWAYS** use `Arc` + atomics instead of `Mutex` wrappers.

## Error Handling

- Backend services return `Result<T, E>` — propagate with `?`.
- UI handlers use `shell_task` / `shell_spawn` helpers.
- **WHEN handling errors, ALWAYS** log them at the boundary where they are ultimately handled, rather than at the propagation layers.
- **Toast on user-facing errors**: `toast_error()` / `toast_warn()`. Use `toast_success()` / `toast_info()` for confirmations.

## File Organization

- Keep files focused: one view per file, one service per file.
- Complex views/services/handlers use directory modules (e.g., `views/settings/mod.rs`, `services/queue/mod.rs`, `update/hotkeys/mod.rs`).
- Handler files in `update/` correspond 1:1 to views, plus specialized handlers for cross-cutting concerns. `ls src/update/` to see them.
- Shared helpers live in `update/components.rs`.

## Core Requirements (WHEN / ALWAYS)

- **WHEN handling an audio track change, ALWAYS** create fresh decoders before locking. Release the audio engine lock during decoder operations.
- **WHEN defining a view's render output, ALWAYS** maintain a consistent root widget type (e.g., keep it a `Column` or `Row`) across renders to ensure `text_input` focus is preserved.
- **WHEN adding a new crate, ALWAYS** discuss it first; rely on the existing workspace dependencies for everyday work. Runtime: `iced`, `tokio`, `tracing` (+ `tracing-subscriber`), `parking_lot`, `futures`, `anyhow`, `image`, `notify`, `mpris-server`, `reqwest`, `serde` (+ `serde_json`), `toml` (+ `toml_edit`), `bincode-next`, `redb`, `chrono`, `directories`, `url`, `httpdate`, `rand`, `lru`, `bytemuck`, `font-kit`, `rodio`, `ringbuf`, `rustfft`, `num-complex`, `biquad`, `symphonia`, `icy-metadata`, `color-thief`, `thiserror`, `pipewire` (linux-only). Test-only `[dev-dependencies]`: `proptest`, `tempfile`.
- **WHEN implementing search behavior, ALWAYS** fire queries immediately on text change rather than debouncing.
- **WHEN working with the visualizer FFT thread, ALWAYS** use `try_lock()`. Only the main render thread may use `lock()`.
- **WHEN handling play actions in update routines, ALWAYS** protect against split-view conflicts by calling `guard_play_action()` at the top of the handler.

## Formatting

- **All code must pass `cargo +nightly fmt --all`**. Config in `rustfmt.toml` (100-char max, crate-level import merging, std/external/crate import grouping).

## Testing & Verification

### Red-Green TDD Protocol

**WHEN implementing bug fixes or new feature handlers, ALWAYS follow red-green TDD:**

1. **Red**: First, write tests in `src/update/tests.rs` (using the `test_app()` helper) asserting the desired behavior. Tests must use **observable state mutations** (e.g., `modes.random`, `modes.consume`, `search_query`) and ALWAYS target pure state rather than side effects requiring `app_service`. Run tests, confirm they **fail**.
2. **Green**: Second, implement the minimal fix/feature to make the tests pass.
3. **Verify**: Run `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo +nightly fmt --all`.

**WHEN structural plumbing is required** (adding fields, wiring message variants), ALWAYS complete the plumbing first so the tests can compile, but implement no behavioral changes until the tests are red.

**Test placement**: `update/tests.rs` for update handler tests, inline `#[cfg(test)]` modules for self-contained logic (data types, widgets, pure functions).

### CI Commands

```bash
cargo +nightly fmt --all      # Format (nightly required)
cargo clippy --all-targets -- -D warnings   # Lint — enforce zero warnings for CI parity
cargo test                    # Unit tests
cargo build --release         # Release build verification
```

## Config & Persistence

| Store | What | How |
|-------|------|-----|
| `config.toml` | User preferences (General, Interface, Playback, Hotkeys, Views, Visualizer behavior, font_family, library_page_size, artwork render resolutions) | Hot-reloadable via `SettingsManager` & `config_writer.rs`. `verbose_config` mode ensures defaults are output. |
| Theme files | Named `.toml` files in `~/.config/nokkvi/themes/` | Palette colors, visualizer colors. 21 built-in themes. `config.toml` stores `theme = "name"` key. |
| redb | Queue, session tokens (JWT, Subsonic) | Via `state_storage.rs`, `queue/`, `credentials.rs`. Plaintext. |
| Credentials | Server URL, username | Stored in `config.toml`. Password is NOT stored on disk. |

## Rule Maintenance

When a commit changes **architecture, patterns, or conventions**, check if `.agent/rules/` files need updating. Flag stale rules before committing.
