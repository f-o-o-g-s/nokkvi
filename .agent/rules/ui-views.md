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
- `impl_expansion_update!` macro — deduplicates expansion handling (7 common arms for expansion views)
- `synth_set_offset_message(&self, offset: usize) -> Option<Message>` — builds a per-view `SlotList(SlotListPageMessage::SetOffset(offset, default_modifiers))` message; used by `handle_seek_settled` to trigger artwork prefetch after scrollbar seek. Expansion views implement this; Queue and Settings return `None`.

Every per-view `*Message` enum carries a `SlotList(SlotListPageMessage)` variant as the unified slot-list carrier. This replaced the old per-view flat variants (`SetSearch`, `SetScrollOffset`, `NavigateUp`, `NavigateDown`, etc.). All slot-list state mutations route through `SlotListPageMessage`.

Pages on `Nokkvi`: Login, Albums, Artists, Genres, Playlists, Queue, Songs, Radios, Settings, Similar. Similar has no `View` enum variant — it only renders inside the browsing panel.

## SlotListPageState

Shared by every slot-list view: search query, scroll position, focus index. Visible slot count is computed dynamically from window height (always odd, capped at `MAX_SLOT_COUNT = 29`); window resizes propagate via `update/window.rs`. Prefetch radius = `slot_count + MIN_PREFETCH_BUFFER` (3 by default).

**Multi-selection** (Ctrl+click / Shift+click / per-row checkbox / select-all): `selected_indices: HashSet`, `anchor_index` for range. `handle_slot_click()` handles modifier-aware selection; `handle_selection_toggle(offset, total)` and `handle_select_all_toggle(total)` drive the optional checkbox column. `select_all_state(total)` returns the tri-state (`None` / `Some` / `All`) the header bar uses. `clear_multi_selection()` resets. `evaluate_context_menu()` resolves batch targets for right-click menus.

## Navigation & Interaction

### Root-level SlotListMessage (keyboard + scrollbar)

`SlotListMessage` in `app_message.rs` carries global slot-list actions dispatched by hotkeys and scrollbar timers: `NavigateUp`, `NavigateDown`, `SetOffset(usize)`, `ActivateCenter`, `ToggleSortOrder`, `ScrollbarFadeComplete(View, u64)`, `SeekSettled(View, u64)`. Root dispatch is in `handle_slot_list_message` (`update/slot_list.rs`). Each hotkey arm fans out to a per-view `Message::Albums(AlbumsMessage::SlotList(SlotListPageMessage::NavigateUp))` (and so on for every view), so the actual state mutation is always done by the per-view update handler.

### Per-view SlotList(SlotListPageMessage) carrier

Every per-view message enum carries a unified `SlotList(SlotListPageMessage)` variant (e.g., `AlbumsMessage::SlotList(…)`, `SongsMessage::SlotList(…)`). `SlotListPageMessage` (in `widgets/slot_list_page.rs`) enumerates all slot-list actions: `NavigateUp`, `NavigateDown`, `SetOffset(usize, Modifiers)`, `ScrollSeek(usize)`, `ActivateCenter`, `ClickPlay(usize)`, `SelectionToggle(usize)`, `SelectAllToggle`, `AddCenterToQueue`, `RefreshViewData`, `CenterOnPlaying`, `SearchQueryChanged(String)`, `SearchFocused(bool)`, `SortModeSelected(SortMode)`, `ToggleSortOrder`.

**Non-expansion views** (Songs, Queue, Radios, Similar) call `self.common.handle(msg, total)` → `SlotListPageAction`, then map the action to their `*Action` enum. `SlotListPageState::handle()` is the unified dispatcher.

**Expansion views** (Albums, Artists, Genres, Playlists) match `SlotList(msg)` sub-variants individually inside their update's `Err(msg)` arm (after `impl_expansion_update!` handles search/sort/expand variants), because navigation must route through expansion-aware methods like `expansion.handle_navigate_up()` / `expansion.handle_select_offset()`.

`dispatch_view_with_seek!` macro (in `update/mod.rs`) wraps each view's `handle_*` call: it detects if the message was a `SlotList(SlotListPageMessage::ScrollSeek(_))` and, if so, appends `scrollbar_fade_timer` + `seek_settled_timer` tasks.

- Non-wrapping navigation; dynamic center slot near edges.
- **Stable viewport** (default): non-center clicks → `SetOffset` (highlight in-place); center clicks → `ActivateCenter`.
- **Legacy mode**: non-center clicks → `ClickPlay` (direct play).
- Activation flash: `slot_list.flash_center()` on activation/transitions.
- Clickable text links: inline album/artist text dispatches `NavigateAndFilter(View, LibraryFilter)` via `mouse_area`. When the browsing panel is active, navigation updates its tab instead of switching the main view.
- Clickable star ratings + clickable hearts on every slot via `mouse_area`.
- Scrollbar timers carry the target `View` so seek messages route correctly between panes.

## Inline Expansion

Generic `ExpansionState<C>` + `SlotListEntry<P, C>`. When active, sort/search may target the expansion — check `expansion.is_expanded()`. Center-entry resolution is centralized in `views/expansion.rs`. Shift+Enter on Artists/Genres collapses the outer expansion.

**Find-and-expand** (clicking an inline album/artist/genre link): the chain runs through a single `Nokkvi.pending_expand: Option<state::PendingExpand>` (variants `Album { album_id, for_browsing_pane }`, `Artist { ... }`, `Genre { ... }`). Per-view `try_resolve_pending_expand_*` consume it once the target appears in its library buffer; `PendingTopPin` re-pins the highlight after the matching `set_children` lands. `for_browsing_pane = true` routes the final `FocusAndExpand` into the browsing-panel tab instead of the top pane. `PendingExpand::host_view()` drives the cancel-on-navigation check in `handle_switch_view`.

## Column Visibility (Albums / Artists / Genres / Playlists / Queue / Songs / Similar)

`view_header.rs` exposes a `checkbox_dropdown` of column toggles per view. The dropdown is a controlled overlay — opening it dispatches `Message::SetOpenMenu(Some(OpenMenu::CheckboxDropdown { view, trigger_bounds }))`. Similar lives only inside the browsing panel and lacks a `View::Similar` variant, so it uses its own `OpenMenu::CheckboxDropdownSimilar { trigger_bounds }`. Column flags persist on `PlayerSettings` (`{view}_show_*` fields, including `_select`, `_index`, `_thumbnail`, `_album`, `_genre`, `_stars`, `_default_playlist`, etc.). Stars use responsive hide rather than per-mode toggling.

**Multi-select column**: opt-in `{view}_show_select` flag adds a per-row checkbox + tri-state "select all" header bar to every slot-list view. Helpers `wrap_with_select_column()` and `compose_header_with_select()` (`widgets/slot_list.rs`) keep per-view plumbing minimal; the checkbox state mirrors `selected_indices` regardless of how membership was set.

**Genre column** (Queue): stacks under the album when both columns are visible, takes over the album slot at album-size font when album is hidden. Auto-shows when sort = Genre (mirrors how the plays column auto-shows on MostPlayed sort).

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

Physical sort via `QueueManager::sort_queue()`, persists to redb. `QueueSortMode`: Album, Artist, Title, Duration, Genre, Rating, MostPlayed, Random (re-rolls on re-select / order toggle). Album column visible across all sort modes; stars use responsive hide. Sort signature is cached and `sort_by_cached_key` avoids re-keying when the signature is unchanged.

## Queue Shuffle

Re-shuffles the order array when a shuffled queue with repeat-playlist wraps back to the start, instead of replaying the same shuffle sequence.

## Roulette (slot-machine random pick)

Available on every slot-list view via the "Roulette" entry in the sort dropdown or the `Roulette` hotkey (default `Ctrl+R`). State on `Nokkvi.roulette: Option<state::RouletteState>` is snapshotted at start so live data churn (page loads, search edits, queue mutations) cannot drift the landing target. Tick handlers in `update/roulette.rs` derive the offset purely from elapsed time via a constant-velocity cruise → ease-out-quad deceleration → keyframe fake-out walk. Cancelled by view change or activating a slot.

## Update Handler Pattern

Root dispatch in `update/mod.rs`. `ls src/update/` for handler files. The async-bridge helpers `shell_task` / `shell_spawn` are methods on `Nokkvi` (`src/main.rs`). Cross-cutting helpers:

**`update/chrome.rs`** — shared handler prologue:
- `HasViewChrome` trait — implemented by all 7 library-view message types. Classifies variants as `SetOpenMenu`, `Roulette`, nav-sfx, or expand-sfx.
- `dispatch_view_chrome<M: HasViewChrome>(handler, msg, view)` — run at the top of every `handle_*` function. Returns `Some(task)` for `SetOpenMenu` / `Roulette` intercepts (caller returns immediately); returns `None` for normal page actions (after triggering the appropriate SFX).

**`update/components.rs`** — shared action helpers:
- `guard_play_action` — split-view + playlist-edit conflict guard
- `set_item_rating_task`, `star_item_task`, `radio_mutation_task`
- `handle_common_view_action` — applies generic Search/Sort/Navigate actions to non-Queue library views; called from each view's handler after the page `update()` returns a `CommonViewAction`
- `PaginatedFetch::from_common()` — needs_fetch-gated paginated load (Albums / Artists / Songs)
- `prefetch_album_artwork_tasks` / `prefetch_song_artwork_tasks` — viewport-window artwork prefetch
- `play_entity_task` / `add_entity_to_queue_task` / `insert_entity_to_queue_at_position_task` — generic entity-action builders
- Boilerplate extraction helpers in `widgets/slot_list_page.rs` (`get_queue_target_indices`, `get_batch_target_indices`) and `views/expansion.rs` (`build_batch_payload`)

## View Data Refresh

- **Manual**: header Refresh button or the `RefreshView` hotkey (default `R`) → `set_needs_fetch()` on `PagedBuffer`
- **Automatic**: Navidrome SSE → `update/library_refresh.rs` → ID-anchored background reload that preserves scroll position. The `background: true` flag on loaded messages prevents scroll jumps. Suppressed by `suppress_library_refresh_toasts`.

## Modals

- **Equalizer**: 10-band + presets (`widgets/eq_modal.rs`, `update/eq_modal.rs`). Selecting a preset auto-enables the EQ. Sliders visually reset to 0 dB when disabled.
- **About**: metadata/diagnostics, theme-adaptive logo (`widgets/about_modal.rs`, `update/about_modal.rs`). Includes a Ko-fi tip link. The Commit row hides gracefully when built outside a git context.
- **Info**: Get Info two-column property table (`widgets/info_modal.rs`, `update/info_modal.rs`). `InfoModalItem` variants per type.
- **Text Input**: name/comment edits + confirmations (`widgets/text_input_dialog.rs`).
- **Default Playlist Picker**: modal sub-slot-list to choose the default playlist (`widgets/default_playlist_picker.rs`, `update/default_playlist_picker.rs`). State on `Nokkvi.default_playlist_picker`; opened from the chip in the Playlists/Queue header or the Playback → Playlists settings entry.

All wrapped in an overlay container with `mouse_area` for correct SVG rendering.

## System Tray

`src/services/tray.rs` runs a ksni-based StatusNotifierItem on a dedicated thread. `update/tray.rs` handles `TrayEvent` (toggle window, play/pause, next/prev, quit) and window-close-to-tray when `close_to_tray` is enabled.
