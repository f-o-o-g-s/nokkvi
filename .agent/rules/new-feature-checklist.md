---
trigger: model_decision
description: End-to-end checklist when building new features. Covers cross-view sync, persistence, hotkeys, MPRIS, scrobbling, sort modes, search, artwork.
---

# New Feature Checklist

Use this checklist when implementing a new feature end-to-end to avoid missing integration points.

**Guiding principle**: Always implement the most robust, DRY, and scalable solution. No shortcuts — handle edge cases, race conditions, and error states during initial implementation.

## Data Layer
- [ ] Domain types in `data/src/types/` (iced-free)
- [ ] API endpoints in `data/src/services/api/`
- [ ] Service methods in `data/src/backend/`
- [ ] Persistence: choose redb (structured state) or TOML (user-editable config)
  - Player-level settings → `PlayerSettings` struct + `SettingsManager`
  - Theme/color config → theme files via `config_writer::update_theme_value()`
  - Visualizer behavior config → `config.toml` via `config_writer::update_config_value()` (auto-injects description comments)

## UI Layer
- [ ] View state + Message + Action enums
- [ ] Update handler in `update/{name}.rs`
- [ ] Root dispatch in `update/mod.rs`
- [ ] Artwork prefetch if view displays album art
- [ ] Wrap slot list in `wrap_with_scroll_indicator()` for scroll overlay

## Cross-Cutting Concerns
- [ ] **Cross-view sync**: Does this data appear in multiple views? Star/rating changes must propagate.
- [ ] **Clickable interactions**: Star ratings → `ClickSetRating(usize, usize)`, hearts → `ClickToggleStar(usize)`. Use `set_item_rating_task()` for absolute rating with optimistic UI.
- [ ] **Context menu**: `ContextMenuAction(usize, LibraryContextEntry)` variant. Library views use `LibraryContextEntry`; queue uses `QueueContextEntry`.
- [ ] **Toast notifications**: `toast_success()` / `toast_error()` / `toast_warn()` / `toast_info()` with descriptive messages.
- [ ] **Hotkey bindings**: Add `HotkeyAction` variant if feature needs a keyboard shortcut.
- [ ] **MPRIS**: If playback-related, update `services/mpris.rs`.
- [ ] **Scrobbling**: If track-lifecycle related, check `update/scrobbling.rs`.
- [ ] **Sort mode**: Add `SortMode` variants and persistence. Queue sort is physical.
- [ ] **Search filtering**: Implement inline filtering (immediate, no debounce).
- [ ] **Settings**: Add items in the appropriate `views/settings/items_*.rs` file: `items_general.rs` (Application, Mouse, Account, Cache), `items_interface.rs` (Layout, Metadata Strip), `items_playback.rs` (Playback, Scrobbling, Playlists), `items_hotkeys.rs`, `items_theme.rs` (theme picker/presets, font, appearance, palette colors), `items_visualizer.rs`. Use `SettingMeta` with `subtitle` for auto-documenting TOML comments. General/Playback/Interface settings use `WriteGeneralSetting` action.
- [ ] **Stable viewport**: Route non-center clicks through `handle_select_offset` when stable viewport is on.
- [ ] **Playlist edit guard**: Wrap play actions with `guard_play_action()`.
- [ ] **Browsing panel**: If view should be accessible, ensure lazy data loading on tab switch.
- [ ] **HasCommonAction trait**: Implement for the view's Action enum if it has SearchChanged/SortModeChanged/SortOrderChanged.
- [ ] **Icons**: Use Lucide SVGs from `reference-lucide/icons/` — register in `embedded_svg.rs`.

## Verification
- [ ] `cargo +nightly fmt --all` clean
- [ ] `cargo clippy` clean
- [ ] `cargo test` passing
- [ ] Manual test: happy path + edge cases
- [ ] Check that widget tree structure is stable (no focus loss)
