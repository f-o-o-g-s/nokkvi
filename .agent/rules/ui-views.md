---
trigger: glob
globs: src/views/**,src/update/**
---

# UI Views & Update Handlers

## ViewPage Trait

All slot-list-based views implement `ViewPage` (in `views/mod.rs`) — explicit `impl` per view, no macro:
- `current_view_page() / current_view_page_mut()` — pane-aware routing (delegates to browsing panel in split-view)
- `view_page(View) / view_page_mut(View)` — direct lookup by `View` enum
- `CommonViewAction` + `HasCommonAction` — generic SearchChanged / SortModeChanged / SortOrderChanged dispatch (handled centrally in `handle_common_view_action()`)
- `impl_expansion_update!` macro — deduplicates expansion handling

Pages on `Nokkvi`: Login, Albums, Artists, Genres, Playlists, Queue, Songs, Radios, Settings, Similar.

## SlotListPageState

Shared by every slot-list view: search query, scroll position, focus index. Visible slots: 9 → 7 → 5 → 3 → 1 based on window height. Prefetch radius = `slot_count + MIN_PREFETCH_BUFFER` (3 by default).

**Multi-selection** (Ctrl+click / Shift+click): `selected_indices: HashSet`, `anchor_index` for range. `handle_slot_click()` handles modifier-aware selection. `clear_multi_selection()` resets. `evaluate_context_menu()` resolves batch targets for right-click menus.

## Navigation & Interaction

- `SlotListMessage` sub-enum routes through `handle_slot_list_message` in `update/slot_list.rs`. Non-wrapping, dynamic center slot near edges.
- **Stable viewport** (default): non-center clicks → `handle_select_offset()` (highlight in-place); center clicks → `SlotListActivateCenter`.
- **Legacy mode**: non-center clicks → `SlotListClickPlay` (direct play).
- Activation flash: `slot_list.flash_center()` on activation/transitions.
- Clickable text links: inline album/artist text dispatches `NavigateAndFilter(View, LibraryFilter)` via `mouse_area`. When the browsing panel is active, navigation updates its tab instead of switching the main view.
- Clickable star ratings + clickable hearts on every slot via `mouse_area`.
- Scrollbar timers carry the target `View` so seek messages route correctly between panes.
- `dispatch_view_with_seek!` macro routes `SlotListScrollSeek` messages.

## Inline Expansion

Generic `ExpansionState<C>` + `SlotListEntry<P, C>`. When active, sort/search may target the expansion — check `expansion.is_expanded()`. Center-entry resolution is centralized in `views/expansion.rs`. Shift+Enter on Artists/Genres collapses the outer expansion.

## Column Visibility (Albums / Artists / Songs / Queue)

`view_header.rs` exposes a `checkbox_dropdown` of column toggles per view. The dropdown is a controlled overlay — opening it dispatches `Message::SetOpenMenu(Some(OpenMenu::CheckboxDropdown { view, trigger_bounds }))`. Column flags persist on `PlayerSettings` (`{view}_show_*` fields). Stars columns are always rendered with responsive hide rather than per-mode toggling.

## Context Menus & Toasts

- Library views: `LibraryContextEntry`. Queue: `QueueContextEntry`. Strip: `StripContextEntry`.
- Toast helpers: `toast_info()`, `toast_success()`, `toast_warn()`, `toast_error()`.
- Batch actions: context menu resolves targets via `evaluate_context_menu()` (or generates full-batch payloads for algorithmic views like Similar Songs), then dispatches batch operations. `clear_multi_selection()` after every batch completion.

## Browsing Panel (Split-View)

Toggled via Ctrl+E from Queue. `BrowsingView` enum: `Songs`, `Albums`, `Artists`, `Genres`, `Similar`. Reuses existing page structs. `PaneFocus` toggled via Tab. Play actions blocked via `guard_play_action()`.

**Cross-pane drag** supports batch: `cross_pane_drag_selection_count` tracks single vs multi-selection batch. Drag threshold 5 px. Center index snapshotted at press time.

## Playlist Editing

`PlaylistEditState` for dirty detection. Inline name/comment editing in queue header (stacked vertically). Save via `handle_save_playlist_edits()`. Browsing panel cannot close during edit.

## Queue Sort

Physical sort via `QueueManager::sort_queue()`, persists to redb. `QueueSortMode`: Album, Artist, Title, Duration, Genre, Rating, MostPlayed. Album column visible across all sort modes; stars use responsive hide. Sort signature is cached and `sort_by_cached_key` avoids re-keying when the signature is unchanged.

## Queue Shuffle

Re-shuffles the order array when a shuffled queue with repeat-playlist wraps back to the start, instead of replaying the same shuffle sequence.

## Update Handler Pattern

Root dispatch in `update/mod.rs`. `ls src/update/` for handler files. Common helpers in `update/components.rs`:
- `shell_task` / `shell_spawn` — run async work against `AppService`
- `guard_play_action` — split-view + playlist-edit conflict guard
- `set_item_rating_task`, `radio_mutation_task`
- `handle_common_view_action` — applies generic Search/Sort actions to all 7 non-Queue library views
- `PaginatedFetch::from_common()` — needs_fetch-gated paginated load (Albums / Artists / Songs)
- `prefetch_album_artwork_tasks` / `prefetch_song_artwork_tasks` — viewport-window artwork prefetch
- Boilerplate extraction helpers in `widgets/slot_list_page.rs` (`get_queue_target_indices`, `get_batch_target_indices`) and `views/expansion.rs` (`build_batch_payload`)

## View Data Refresh

- **Manual**: header Refresh button / hotkeys (F5 / Ctrl+R) → `set_needs_fetch()` on `PagedBuffer`
- **Automatic**: Navidrome SSE → `update/library_refresh.rs` → ID-anchored background reload that preserves scroll position. The `background: true` flag on loaded messages prevents scroll jumps. Suppressed by `suppress_library_refresh_toasts`.

## Modals

- **Equalizer**: 10-band + presets (`widgets/eq_modal.rs`, `update/eq_modal.rs`). Selecting a preset auto-enables the EQ. Sliders visually reset to 0 dB when disabled.
- **About**: metadata/diagnostics, theme-adaptive logo (`widgets/about_modal.rs`, `update/about_modal.rs`). Includes a Ko-fi tip link. The Commit row hides gracefully when built outside a git context.
- **Info**: Get Info two-column property table (`widgets/info_modal.rs`, `update/info_modal.rs`). `InfoModalItem` variants per type.
- **Text Input**: name/comment edits + confirmations (`widgets/text_input_dialog.rs`).

All wrapped in an overlay container with `mouse_area` for correct SVG rendering.

## System Tray

`src/services/tray.rs` runs a ksni-based StatusNotifierItem on a dedicated thread. `update/tray.rs` handles `TrayEvent` (toggle window, play/pause, next/prev, quit) and window-close-to-tray when `close_to_tray` is enabled.
