---
paths:
  - "src/widgets/slot_list*.rs"
  - "src/widgets/{base_slot_list_layout,picker_modal}.rs"
  - "src/views/**"
  - "src/update/**"
  - "src/state/{panes,pending,playlist_editor}.rs"
  - "src/app_view.rs"
  - "src/main.rs"
---

# Gotchas: Slot Lists, Selection & Expansion

## Multi-Selection & Batch

- **Ctrl+click toggle**: deselecting the last item must clear `selected_offset` to remove the center-highlight from the deselected slot.
- **Shift+click range**: clears existing selection first, then adds the range from `anchor_index` to clicked offset.
- **Context menu batch**: `evaluate_context_menu()` checks if the clicked index is in the selection; if not, resets selection to just that item.
- **Settle the slot list when rows leave it**: `SlotListPageState::settle_after_shrink(total)` drops the selection, its anchor and the click-to-focus marker (positions into shifted rows) and clamps the viewport. A kept marker snaps the next scroll back to it, past the end, where every slot renders empty. The queue's removals (`drop_queue_rows`) and the editor's go through it.
- **Cross-pane drag batch**: `cross_pane_drag.selection_count` (on the `CrossPaneDragUi` cluster) is decided at drag activation (5 px threshold): a pressed row inside the selection drags the whole selection, otherwise the selection resets to that row. Press itself leaves the selection alone (it would race the row's checkbox toggle).
- **Keyboard scroll drops the click focus**: `SlotListView::move_up/down` drop the click-to-focus marker (and a selection a single click made) so a stale highlight can't linger; checkbox multi-selections survive, and under a find-and-expand top-pin the marker moves with the viewport instead.
- **Which rows a view shows under a search**: the library views (Albums, Artists, Songs, Genres, Playlists) search on the server and render their buffer as it came back, so a question about their rows reads the buffer; only the Queue and Radios filter client-side (`filter_queue_songs` / `filter_radio_stations`, which ARE their render rules and their slot-index maps), and the playlist editor renders its whole buffer (no search box reaches it). A client re-filter of a library buffer disagrees with the server wherever its matching does (Navidrome folds accents); `focused_list_is_empty` is the pattern.
- **Search clears the whole selection**: `handle_search_query_changed` (every slot-list view's search, Harbour's mirror included) drops indices + anchor + focus marker, because indices are relative to the filtered list and a non-empty set suppresses the center ring. A kept click left the filtered rows with no highlight.
- **So does every other library-view reorder or buffer replace**: `clear_selection_for_refresh()` (all three fields), never `set_offset` alone (focus marker only) or `clear_multi_selection` alone (keeps the marker). The queue's reload, sort and random-shuffle paths (`handle_queue_loaded`, `apply_queue_sort`, `dispatch_random_queue_shuffle`) call `clear_multi_selection` instead and keep the marker (queue.md "Stale multi-selection across refreshes"). Both branches of `apply_viewport_on_load` (`src/update/loader_target.rs`) clear: the foreground branch too, because `get_effective_center_index` prefers the marker, so a sort change left Enter / Shift+Q acting on an off-screen row. The clear is safe for find-and-expand: `handle_loaded_with` runs `apply_viewport_on_load` BEFORE `try_resolve_pending_expand`, so the chain's `pin_selected` lands after it (pinned by `albums_foreground_reload_still_pins_a_pending_find_and_expand`). Genres and Playlists override `apply_viewport_on_load` and need no clear — their post-load `SetOffset(0)` click replaces the selection.

## Expansion, Drag & Viewport

- **Paged viewport clamp**: a background refresh that can't find the viewport's row keeps `viewport_offset`, clamped to the new length (Albums, Artists, Songs) — otherwise the viewport can land past the end after a remove, or jump to the top.
- **Expansion sort state**: when expansion is active, sort/search may target the expansion. Check `expansion.is_expanded()`. Shift+Enter on Artists/Genres collapses the outer expansion.
- **Pending find-and-expand chain**: at most one `Nokkvi.pending_expand.target` (on the `PendingExpandState` cluster) runs at a time. Starting a new chain (or any user-driven view change matching `PendingExpand::host_view()`) supersedes the previous one. `PendingTopPin` re-pins the highlight after `set_children` lands.
- **Expansion artwork retry**: artwork fetches dispatched from inline expansions retry on transient failure and reject empty-bytes responses, so a flaky first request doesn't leave a permanent empty cell.
- **Cross-pane drag center index**: the row hovered at press (`pressed_item`), copied into the drag at activation — decoupled from later state changes.
