---
trigger: glob
globs: src/views/**,src/update/**
---

# UI Views & Update Handlers

## ViewPage Trait

All slot-list-based views implement `ViewPage` (in `views/mod.rs`):
- Explicit `impl ViewPage for XPage` blocks (no macro)
- `current_view_page()` / `current_view_page_mut()` — pane-aware routing (delegates to browser pane in edit mode)
- `view_page(View)` / `view_page_mut(View)` — direct lookup by `View` enum, no pane routing
- `CommonViewAction` + `HasCommonAction` for generic SearchChanged/SortModeChanged/SortOrderChanged
- `impl_expansion_update!` macro for expansion deduplication

## SlotListPageState

All views use `SlotListPageState`: search query, scroll position, focus index. Visible slots: 9→7→5→3→1 based on window height. 23-slot artwork prefetch window.

**Multi-selection** (Ctrl+click / Shift+click): `selected_indices` (HashSet), `anchor_index` for range selection. `handle_slot_click()` handles modifier-aware selection. `clear_multi_selection()` resets. `evaluate_context_menu()` resolves batch targets for right-click menus.

## Navigation & Interaction

- `SlotListMessage` sub-enum via `handle_slot_list_message` in `update/slot_list.rs`. Non-wrapping, dynamic center slot near edges.
- **Stable viewport** (default): non-center clicks → `handle_select_offset()` (highlight in-place). Center clicks → `SlotListActivateCenter`.
- **Legacy mode**: non-center clicks → `SlotListClickPlay` (direct play)
- Activation flash: `slot_list.flash_center()` on activation/transitions
- Clickable text links: inline album/artist text routing to respective views via `NavigateAndFilter(View, LibraryFilter)` through `mouse_area` overlays. When browsing panel is active, navigation updates the panel's internal tab instead of switching the main view.
- Clickable star ratings + clickable hearts on all slots via `mouse_area`.
- Scrollbar timers carry target `View` — fixes browsing panel routing.
- `dispatch_view_with_seek!` macro handles `SlotListScrollSeek` messages

## Inline Expansion

Generic `ExpansionState<C>` + `SlotListEntry<P, C>`. When active, sort/search may target expansion — check `expansion.is_expanded()`.

## Context Menus & Toasts

- Library views: `LibraryContextEntry`. Queue: `QueueContextEntry`. Strip: `StripContextEntry`.
- Toast helpers: `toast_info()`, `toast_success()`, `toast_warn()`, `toast_error()`
- Batch actions: context menu resolves `evaluate_context_menu()` for multi-selection (or generates full-batch payloads for algorithmic views like Similar Songs), then dispatches batch operations. `clear_multi_selection()` after batch completion.

## Browsing Panel (Split-View)

Toggled via Ctrl+E from Queue. `BrowsingView` enum: Albums, Songs, Artists, Genres, Similar. Reuses existing page structs. `PaneFocus` toggled via Tab. Play actions blocked via `guard_play_action()`.

**Cross-pane drag** supports batch: `cross_pane_drag_selection_count` tracks whether dragging a single item or a multi-selection batch. Drag threshold 5px. Center index snapshotted at press time.

## Playlist Editing

`PlaylistEditState` for dirty detection. Inline name/comment editing in queue header (stacked vertically). Save via `handle_save_playlist_edits()`. Browsing panel cannot close during edit.

## Queue Sort

Physical sort via `QueueManager::sort_queue()`, persists to redb. Album column visible across all sort modes. Stars column is always rendered and uses responsive hide (collapses with the rest of the columns at narrow widths) rather than being toggled per sort mode.

## Queue Shuffle

Shuffle on repeat-playlist wrap: re-shuffles the order array when the queue loops back to the start instead of replaying the same shuffle sequence.

## Update Handler Pattern

Root dispatch in `update/mod.rs`. `ls src/update/` for handler files. Common helpers in `update/components.rs`: `shell_task`, `shell_spawn`, `guard_play_action`, `set_item_rating_task`, `radio_mutation_task`, `handle_common_view_action` (applies generic Search/Sort actions to all 7 non-Queue library views). Boilerplate extraction helpers in `widgets/slot_list_page.rs` (`get_queue_target_indices`) and `views/expansion.rs` (`build_batch_payload`).

## View Data Refresh

View data can be refreshed via:
- **Manual**: header Refresh button / hotkeys (F5 / Ctrl+R) → `set_needs_fetch()` on `PagedBuffer`
- **Automatic**: Navidrome SSE events → `update/library_refresh.rs` → background reload with ID-based anchor to preserve scroll position. The `background: true` flag on loaded messages prevents scroll jumps.
## Modals

- **Equalizer**: 10-band + presets (`widgets/eq_modal.rs`, `update/eq_modal.rs`). Selecting preset auto-enables EQ. Sliders visually reset to 0 dB when disabled.
- **About**: metadata/diagnostics, theme-adaptive logo (`widgets/about_modal.rs`, `update/about_modal.rs`)
- Both wrapped in overlay container with `mouse_area` for correct SVG rendering.
