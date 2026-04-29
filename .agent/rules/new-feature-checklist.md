---
trigger: model_decision
description: End-to-end checklist when building new features. Covers cross-view sync, persistence, hotkeys, MPRIS, scrobbling, sort modes, search, artwork, settings.
---

# New Feature Checklist

## Data Layer
- [ ] Domain types in `data/src/types/` (iced-free)
- [ ] API endpoints in `data/src/services/api/`
- [ ] Service methods in `data/src/backend/`
- [ ] Persistence: redb (structured state) or TOML (user-editable config)
- [ ] Batch-aware actions: `BatchPayload` / `BatchItem` from `data/src/types/batch.rs`

## UI Layer
- [ ] State / Message / Action enums (TEA pattern)
- [ ] Update handler in `update/{name}.rs`, dispatch wired in `update/mod.rs`
- [ ] Paginated loads use `PaginatedFetch::from_common()` (needs_fetch gating built in)
- [ ] Artwork prefetch dispatched from `update/window.rs` if the view shows art
- [ ] Slot list wrapped in `wrap_with_scroll_indicator()`
- [ ] Multi-selection: `handle_slot_click()` + `evaluate_context_menu()` for batch resolution

## Cross-Cutting
- [ ] **Cross-view sync**: star/rating/play-count changes propagate across views
- [ ] **Context menu**: `LibraryContextEntry` / `QueueContextEntry` / `StripContextEntry`
- [ ] **Toasts**: `toast_success()` / `toast_error()` / `toast_warn()` / `toast_info()`
- [ ] **Hotkeys**: add a `HotkeyAction` variant if needed
- [ ] **MPRIS**: update `services/mpris.rs` for playback-related changes
- [ ] **Scrobbling**: check `update/scrobbling.rs` for track-lifecycle hooks
- [ ] **Sort/Search**: extend `SortMode` (or `QueueSortMode`); search is immediate (no debounce)
- [ ] **Settings**: add entries in `views/settings/items_*.rs` with `SettingMeta` (subtitle required)
- [ ] **Config write routing**: settings → `ConfigKey::AppScalar` / `AppArrayEntry` / `Theme` / `ThemeArrayEntry` (typed dispatch in `config_writer.rs`)
- [ ] **Playlist edit guard**: `guard_play_action()` on every play handler
- [ ] **HasCommonAction**: implement on the action enum if the view has SearchChanged/SortModeChanged/SortOrderChanged
- [ ] **Single-active overlay menu**: hamburger / kebab / dropdown / context menus bubble `Message::SetOpenMenu(Some(OpenMenu::…))` instead of owning local `is_open` state
- [ ] **Icons**: drop SVGs into `assets/icons/` — `build.rs` regenerates the lookup table; no manual registration

## Verification
- [ ] **TDD**: write tests for observable state mutations *before* implementing handlers (`update/tests.rs`, `test_app()` helper)
- [ ] `cargo +nightly fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test` clean
- [ ] Manual: happy path + edge cases + stable widget tree (root widget type unchanged across renders)
