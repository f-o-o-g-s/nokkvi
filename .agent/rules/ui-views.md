---
trigger: glob
globs: src/views/**,src/update/**
---

# UI Views & Update Handlers

## ViewPage Trait & Common Actions

All slot-list-based views implement `ViewPage` (in `views/mod.rs`):
- Explicit `impl ViewPage for XPage` blocks (no macro)
- Queue is hand-implemented (separate `QueueSortMode`, unique actions)
- `current_view_page()` / `current_view_page_mut()` — pane-aware routing (delegates to browser pane in edit mode)
- `view_page(View)` / `view_page_mut(View)` — direct lookup by `View` enum, no pane routing (used by scrollbar timers)

`CommonViewAction` + `HasCommonAction` trait enable generic handling of SearchChanged, SortModeChanged, SortOrderChanged, and None. `impl_expansion_update!` macro deduplicates common expansion update arms.

## SlotListPageState

All views (Albums, Artists, Songs, Genres, Playlists, Queue) use `SlotListPageState` for shared state:
- Search query, scroll position, focus index
- Visible slots: 9→7→5→3→1 based on window height
- 23-slot artwork prefetch window

## Slot List Navigation

- `SlotListMessage` sub-enum dispatched via `handle_slot_list_message` in `update/slot_list.rs`
- **Non-wrapping**: clamped at list boundaries
- **Dynamic center slot**: active item position adapts near list edges
- `get_effective_center_index` respects `selected_offset`
- `SlotListRowContext` bundles per-slot render args
- **Scrollbar timers**: `ScrollbarFadeComplete(View, u64)` and `SeekSettled(View, u64)` carry the target `View` — fixes browsing panel where `current_view` is Queue but the scrollbar is on a library view
- `dispatch_view_with_seek!` macro handles `SlotListScrollSeek` messages: calls the view handler, then appends `scrollbar_fade_timer(view)` + `seek_settled_timer(view)` for seek events

## Click Behavior

- **Stable viewport mode** (default): non-center clicks call `handle_select_offset()` — highlights in-place
- **Legacy mode**: non-center clicks dispatch `SlotListClickPlay(item_index)` — direct play
- Center clicks always dispatch `SlotListActivateCenter`
- **Activation flash**: `slot_list.flash_center()` on activation, next/prev transitions
- **Clickable star ratings** and **clickable hearts** on all slots via `mouse_area`

## Inline Expansion

- Generic `ExpansionState<C>` + `SlotListEntry<P, C>` for drill-down within a view
- `ExpansionState::handle_select_offset()` — click-to-focus variant
- When expansion is active, sort/search operations may target the expansion — check `expansion.is_expanded()`

## Context Menus

- **Library views**: `LibraryContextEntry` enum with separators
- **Queue view**: `QueueContextEntry` enum
- All fire `ContextMenuAction(item_index, entry)` on the view's message enum

## Toast Notifications

- `ToastMessage` sub-enum: `Push(Toast)`, `PushThen(Toast, Box<Message>)`, `Dismiss`, `DismissKey(String)`
- Helpers: `toast_info()`, `toast_success()`, `toast_warn()`, `toast_error()`
- Descriptive messages with item names

## Browsing Panel (Split-View)

- **Toggled via Ctrl+E** from Queue view
- `BrowsingView` enum: Songs, Albums, Artists, Genres (tab bar order)
- **Reuses existing page structs** — no duplicated logic
- Tab switching triggers lazy data load if needed
- `PaneFocus` enum (Queue | Browser) toggled via Tab key
- **Play actions blocked** — `guard_play_action()` returns toast warning

## Playlist Editing (Split-View)

- `PlaylistEditState` tracks snapshot for dirty detection (`is_dirty()`, `is_name_dirty()`, `is_comment_dirty()`)
- Inline playlist name and comment editing in queue header (name + comment text inputs **stacked vertically** in a `column!`)
- Save via `handle_save_playlist_edits()` → rename + update comment + replace tracks
- Browsing panel cannot be closed during edit mode

## Cross-Pane Drag-and-Drop

- `CrossPaneDragState`: tracks origin, cursor position, snapshotted center_index, drop_target_slot
- State machine: Press → threshold (5px) → active drag → release/cancel
- Drop inserts at position via `pending_queue_insert_position`
- **Scrollbar guard**: global `event::listen_with` filters `CursorMoved` by `event::Status::Captured` — scrollbar captures cursor moves during drag, preventing the 5px threshold from being exceeded. `ButtonPressed`/`ButtonReleased` are NOT filtered (Iced buttons also capture those).

## Artwork Prefetch & Pagination

- `needs_fetch(viewport_offset)` triggers `LoadPage` for next page
- **Centralized artwork prefetch**: `update/window.rs` dispatches across all views on resize/load

## Update Handler Pattern

Each `update/{name}.rs` handles data loading and message routing:
- Root dispatch in `update/mod.rs`
- Handler file listing: see `code-standards.md` File Organization section
- Common helpers in `update/components.rs`: `shell_action_task`, `shell_fire_and_forget_task`, `star_item_task`, `handle_common_view_action`, `set_item_rating_task`, `guard_play_action`, `handle_show_in_folder`

### Settings View Navigation

- `pre_settings_view` tracks active view before entering Settings
- Closing Settings restores previous view
- `SlotListDown` (Tab) unfocuses settings search field

### Play Next Shuffle Warning

- `toast_warn()` when "Play Next" used with shuffle active (all views that support it)

## Cross-View Sync

- Star/rating changes propagate across all views via `update/hotkeys/star_rating.rs`
- Starring (love) auto-sets rating to 5 stars

## Search Filtering

- Fires immediately on query change (no debounce)
- Uses `Searchable` trait for reusable filtering
- Unique search input ID per view

## Queue Sort

- **Physical**: `QueueManager::sort_queue()` reorders in place, persists to redb
- Sort modes: Album, Artist, Title, Duration, Genre, Rating

## Progressive Queue Loading

- First page (~500) plays immediately; `ProgressiveQueueAppendPage` chain fetches rest
- `progressive_queue_generation` prevents stale chains
- `queue_loading_target` drives "X of Y songs" header display

## Playlist Header Bar

- Read-only context bar (32px): list-music accent icon, playlist name + optional comment (stacked), quick-save + edit buttons
- Edit mode bar (44px): pencil accent icon, `column![name_input, comment_input]`, save + discard buttons
- Both bars use `bg0_soft` background + 1px horizontal separator between bar and view header
- Artwork column separator: `with_left_stripe()` adds 2px `bg1` stripe on the artwork panel's left edge
- `active_playlist_info`: 3-tuple `(playlist_id, playlist_name, comment)` — persisted across restarts via `SettingsManager`
- Chrome height: `+33px` for context bar, `+45px` for edit bar (bar height + 1px separator)
- **Quick-save** opens `SaveAsPlaylist` dialog for confirmation
- `active_playlist_info` cleared when non-playlist content replaces the queue (including Shift+D queue clear)

## ShowInFolder

- `Message::ShowInFolder(String)` → `handle_show_in_folder` in `update/components.rs`
- Requires `local_music_path` configured; shows toast warning if unset
- **Songs**: direct path from `SongUIViewData.path`
- **Albums/Artists**: async API fetch via `load_album_songs()` → first song's path → `Message::ShowInFolder`
- Context menu: `LibraryContextEntry::ShowInFolder` — Songs, Albums, Artists views use `library_entries_with_folder()`
- Info modal: `FetchAndOpenAlbumFolder(album_id)` message triggers async fetch for albums without a pre-loaded path

## Modals

- **Equalizer Modal**: 10-band graphic equalizer with interactive sliders (`widgets/eq_slider.rs`). Opened via `Q` hotkey or player bar EQ button. Widget: `widgets/eq_modal.rs`, handler: `update/eq_modal.rs`. Built-in presets + user-saved custom presets.
- **About Modal**: App metadata and diagnostics, accessible via hamburger menu. Widget: `widgets/about_modal.rs`, handler: `update/about_modal.rs`.
- Both modals are wrapped in an overlay container ensuring SVG icon rendering is correct using `mouse_area`.
