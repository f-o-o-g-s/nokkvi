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
- [ ] Async work bridged through `Nokkvi::shell_task` / `shell_spawn` (defined on the root impl in `src/main.rs`)
- [ ] Paginated loads use `PaginatedFetch::from_common()` (needs_fetch gating built in). For new paged entity types: implement `LoaderTarget` in `update/loader_target.rs` and route through `Nokkvi::load_paged::<T>`
- [ ] Artwork prefetch dispatched from `update/window.rs` if the view shows art
- [ ] Slot list wrapped in `wrap_with_scroll_indicator()`
- [ ] Multi-selection: `handle_slot_click()` + `evaluate_context_menu()` for batch resolution. For new slot-list views, wire the optional checkbox column via `wrap_with_select_column()` / `compose_header_with_select()` and add a `{view}_show_select` toggle

## Cross-Cutting
- [ ] **Cross-view sync**: star/rating/play-count changes propagate across views
- [ ] **Context menu**: `LibraryContextEntry` / `QueueContextEntry` / `StripContextEntry`
- [ ] **Toasts**: `toast_success()` / `toast_error()` / `toast_warn()` / `toast_info()`
- [ ] **Hotkeys**: add a variant to the `define_hotkey_actions!` table in `data/src/types/hotkey_config/action.rs` (it emits the enum, `ALL` / `RESERVED` slices, default-binding, and TOML wire string from one declaration)
- [ ] **MPRIS**: update `services/mpris.rs` for playback-related changes
- [ ] **Scrobbling**: check `update/scrobbling.rs` for track-lifecycle hooks
- [ ] **Sort/Search**: extend `SortMode` (or `QueueSortMode`); search is immediate (no debounce)
- [ ] **Settings**: every General / Interface / Playback / Visualizer knob is one `define_settings!` entry in `data/src/services/settings_tables/` — the schema row emits the dispatch arm, the persistence round-trips, and (via `ui_meta`) the UI row (details in `settings-view.md`). Theme / Hotkey items and the visualizer color sections still build by hand in `views/settings/items_*.rs` using `SettingMeta::new(key, label, category).with_subtitle(...)` (subtitle is optional). New settings rows get curated search synonyms in `data/src/utils/setting_keywords.rs::keywords_for` so the fuzzy settings search finds them by alias terms
- [ ] **Config write routing**: settings → `ConfigKey::AppScalar` / `Theme` / `ThemeArrayEntry` (typed dispatch in `config_writer.rs`). Sentinel pseudo-keys (logout, restore-defaults, the ListenBrainz/Last.fm credential actions) route through `SentinelKind` in `views/settings/sentinel.rs`
- [ ] **Pre-play hook**: `guard_play_action()` on every play handler (transitions radio playback → queue mode; no longer blocks playlist edits)
- [ ] **HasCommonAction**: implement on the action enum if the view has SearchChanged/SortModeChanged/SortOrderChanged
- [ ] **Single-active overlay menu**: hamburger / kebab / dropdown / context menus bubble `Message::SetOpenMenu(Some(OpenMenu::…))` instead of owning local `is_open` state
- [ ] **Icons**: drop the Lucide SVG into `assets/icons/` — `build.rs` walks both `assets/icons/` and `assets/icons-phosphor/` and regenerates the lookup table (no manual `const`/match edits). Phosphor is the **default** icon set, so if a view references the new stem, also add a `NAME_MAP` entry (Lucide stem → Phosphor path) in `src/embedded_svg.rs` and ship the matching file under `assets/icons-phosphor/`; otherwise `get_svg()` silently falls through to the Lucide glyph under the default set. A path referenced in code but missing on disk falls back to `play.svg` — `cargo test --bin nokkvi -- embedded_svg` catches it

## Verification
- [ ] **TDD**: write tests for observable state mutations *before* implementing handlers (`update/tests/{area}.rs` or the per-area `tests_*.rs` siblings; `test_app()` from `src/test_helpers.rs`)
- [ ] **All four CI gates clean** (CI fails any of them): `cargo +nightly fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`, `cargo build --release`
- [ ] **Changelog**: add a user-facing entry under `## [Unreleased]` in `CHANGELOG.md` (the `/commit` skill refreshes it in the same commit; `docs-changelog-sync.yml` rebuilds the docs site on CHANGELOG changes)
- [ ] Manual: happy path + edge cases + stable widget tree (root widget type unchanged across renders)
