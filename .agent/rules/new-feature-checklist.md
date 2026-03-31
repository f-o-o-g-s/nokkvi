---
trigger: model_decision
description: End-to-end checklist when building new features. Covers cross-view sync, persistence, hotkeys, MPRIS, scrobbling, sort modes, search, artwork.
---

# New Feature Checklist

## Data Layer
- [ ] Domain types in `data/src/types/` (iced-free)
- [ ] API endpoints in `data/src/services/api/`
- [ ] Service methods in `data/src/backend/`
- [ ] Persistence: redb (structured state) or TOML (user-editable config)

## UI Layer
- [ ] View state + Message + Action enums (follow TEA pattern)
- [ ] Update handler in `update/{name}.rs`, root dispatch in `update/mod.rs`
- [ ] Artwork prefetch in `update/window.rs` if view displays album art
- [ ] Wrap slot list in `wrap_with_scroll_indicator()`

## Cross-Cutting
- [ ] **Cross-view sync**: star/rating changes propagate across all views
- [ ] **Context menu**: `LibraryContextEntry` (library) or `QueueContextEntry` (queue)
- [ ] **Toasts**: `toast_success()` / `toast_error()` / `toast_warn()` / `toast_info()`
- [ ] **Hotkeys**: `HotkeyAction` variant if needed
- [ ] **MPRIS**: update `services/mpris.rs` if playback-related
- [ ] **Scrobbling**: check `update/scrobbling.rs` if track-lifecycle related
- [ ] **Sort/Search**: `SortMode` variants + persistence. Search is immediate (no debounce).
- [ ] **Settings**: add items in `views/settings/items_*.rs`. Use `SettingMeta` with `subtitle`.
- [ ] **Playlist edit guard**: `guard_play_action()` on play actions
- [ ] **HasCommonAction**: implement if view has SearchChanged/SortModeChanged/SortOrderChanged
- [ ] **Icons**: Lucide SVGs from `reference-lucide/icons/` → register in `embedded_svg.rs`

## Verification
- [ ] `cargo +nightly fmt --all`, `cargo clippy`, `cargo test` clean
- [ ] Manual test: happy path + edge cases + stable widget tree
