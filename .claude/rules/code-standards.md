# Code Standards

## Design Philosophy

- **Prefer the most robust, DRY, scalable solution.** Implement comprehensive fixes designed for the long term.
- **Reuse existing patterns.** Check the codebase before building something new.
- **Handle edge cases proactively.** Address race conditions, error states, and boundary cases during initial implementation.

## Rust Conventions

- **Production error handling**: use `?`, `unwrap_or_default()`, or explicit match — **no `.unwrap()`** in production paths. Enforced by the `unwrap_used = "deny"` workspace lint; tests opt out via `#![cfg_attr(test, allow(clippy::unwrap_used, clippy::print_stderr))]` at each crate root.
- **No `println!` / `dbg!` / `todo!()` / `unimplemented!()` / `mem::forget`**: all `deny` at workspace level. Use `tracing` macros for output; prefer `*_or_else` over `*_or` for non-trivial fallbacks (`or_fun_call = "deny"`); `async fn` without `.await` is rejected (`unused_async = "deny"`); enum matches must enumerate every variant rather than `_ =>` (`match_wildcard_for_single_variants = "deny"`); `assert!(<const expr>)` belongs in a `const _: () = assert!(…)` block (`assertions_on_constants = "deny"`). Don't paper over with broader allows — fix at the call site.
- **Logging**: stderr is quiet by default; the file log at `~/.local/state/nokkvi/nokkvi.log` stays verbose.
- **Threading**: theme color reads use `ArcSwap` (lock-free).

## Error Handling

- Backend services return `Result<T, E>` — propagate with `?`.
- UI handlers run async `AppService` work through `shell_task` / `shell_spawn` (methods on `Nokkvi` in `src/main.rs`).
- Log at the boundary that finally handles, not at every propagation layer.
- User-facing errors get `toast_error()` / `toast_warn()`. Confirmations get `toast_success()` / `toast_info()`.

## File Organization

- One view per file, one service per file.
- Complex views/services/handlers use directory modules (e.g., `views/settings/mod.rs`, `services/queue/mod.rs`, `update/hotkeys/mod.rs`).
- Handler files in `update/` correspond 1:1 to views, plus specialized handlers for cross-cutting concerns. `ls src/update/` to see them.
- `shell_task` / `shell_spawn` are methods on `Nokkvi` (`src/main.rs`) — they bridge UI handlers to async `AppService` work.
- Shared action/entity/artwork-prefetch helpers live in `update/components/` (`mod.rs` + `artwork_prefetch.rs`) — `ls` it rather than trusting a list here. For auth/session work: `session_expired_message(&anyhow::Error) -> Option<Message>` collapses the prior inline 401 downcasts, and `Nokkvi::reset_session_state(&mut self) -> Task<Message>` is the single source for the full logout + session-expired teardown (engine, task manager, library/queue/scrobble caches, modals).

### File placement & naming

| Convention | Example |
|------------|---------|
| View pages | `{Name}Page`, `{Name}Message`, `{Name}Action`, `{Name}ViewData` |
| Update handlers | `update/{name}.rs` |
| Backend services | `data/src/backend/{name}.rs` |
| API endpoints | `data/src/services/api/{name}.rs` |
| Domain types | `data/src/types/{name}.rs` |
| Slot list widgets | `widgets/slot_list.rs` (rendering + `SlotListRowMetrics`), `slot_list_view.rs` (scroll), `slot_list_page.rs` (page state) |

## Core Requirements

Engine lock discipline, root-widget stability, immediate search, FFT `try_lock()`, `guard_play_action()`, and the radius scale helpers are carried by CLAUDE.md (canonical). Beyond those:

- **Border radii**: player chrome uses the `*_player` variants of the scale helpers. `ui_border_radius()` is the legacy single-radius value kept for back-compat — new code calls the scale helper directly.
- **Single-active overlay menus**: hamburger / kebab / checkbox-dropdown / context menus bubble `Message::SetOpenMenu(...)` to root rather than owning local `is_open` state.

## Dependencies

Discuss before adding new crates. The authoritative dependency list is the three Cargo.tomls (workspace root, data/, nokkvi-ipc/) plus the summary in CLAUDE.md. nokkvi-ipc must stay iced-free — the client path links it before iced exists. rfd is pinned to the xdg-portal backend only.

## Formatting

- **All code must pass `cargo +nightly fmt --all`**. Config in `rustfmt.toml` (100-char max, crate-level import merging, std/external/crate import grouping). Nightly is required because of the unstable settings.

## Testing & Verification

TDD protocol + the four CI gates: see CLAUDE.md (canonical).
Test placement: `update/tests/` or `update/tests_*.rs` siblings for handler tests; inline `#[cfg(test)] mod tests` for self-contained logic.

## Config & Persistence

Config/persistence routing (config.toml vs theme files vs redb, ConfigKey variants, verbose_config): see backend-services.md + gotchas.md "Config & Persistence". Adding a user setting goes through the `define_settings!` schema tables (`data/src/services/settings_tables/`) — one row owns dispatch, persistence round-trips, and the UI row; see settings-view.md.

## Release & Versioning

- **`Cargo.toml` version bumps go through `/package` only.** Editing the `version = ` line directly and committing skips the changelog generation, README freshness check, annotated-tag policy, and (on minor/major bumps) the prior-minor archive step. The full procedure lives at `.agent/workflows/package.md`; defer to it for any version-bump commit.
- **Pre-commit enforces the archive step.** When a staged `Cargo.toml` shows a minor or major bump, `.githooks/pre-commit` refuses the commit if the previous minor's release blocks still live in `CHANGELOG.md` — they belong in `changelog-archive/CHANGELOG-X.Y.md`. The hook only runs when `core.hooksPath` points at `.githooks`; `/package` step 0 sets that idempotently, so fresh clones / worktrees inherit the gate once the agent reaches step 0.
- **CI is the backstop, not the primary gate.** `.github/workflows/release.yml` re-runs the same archive check on tag push, but it fails late (after the tag is on origin). Rely on the pre-commit hook for early-catch; the CI gate is what catches `--no-verify` or out-of-`/package` bump paths.

## Rule Maintenance

When a commit changes **architecture, patterns, or conventions**, check `.claude/rules/` for staleness. Flag stale rules before committing.
