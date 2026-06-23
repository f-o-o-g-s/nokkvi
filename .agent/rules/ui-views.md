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
- `impl_expansion_update!` macro — deduplicates expansion handling (12 common arms for expansion views: 7 named arms — expand_center, collapse, children_loaded, sort_selected, toggle_sort, search_changed, search_focused — plus 5 `slot_list_wrap` arms: `HoverEnterSlot` / `HoverExitSlot` and the three `Toolbar*` reveal-lock variants)
- `synth_set_offset_message(&self, offset: usize) -> Option<Message>` — builds a per-view `SlotList(SlotListPageMessage::SetOffset(offset, default_modifiers))` message; used by `handle_seek_settled` to trigger artwork prefetch after scrollbar seek. Six views override it (the four expansion views plus Songs and Radios); Queue and Similar fall through to the trait default `None`; Settings doesn't implement `ViewPage` at all.
- `reload_message(&self) -> Option<Message>` — emits the view's "reload from server" message (the `R` hotkey + manual refresh button bind through this). Reloadable views return `Some(...)`; views without server backing return `None`. Replaces the prior `RefreshView` 7-arm match in update routing.
- `slot_list_message(&self, msg: SlotListPageMessage) -> Message` — wraps a `SlotListPageMessage` in the view's per-view `*Message::SlotList(...)` variant. Lets cross-view dispatch (slot_list.rs, roulette.rs, center-on-playing) fan out without a manual match on `View`.

Every per-view `*Message` enum carries a `SlotList(SlotListPageMessage)` variant as the unified slot-list carrier. This replaced the old per-view flat variants (`SetSearch`, `SetScrollOffset`, `NavigateUp`, `NavigateDown`, etc.). All slot-list state mutations route through `SlotListPageMessage`.

Pages on `Nokkvi`: Login, Albums, Artists, Genres, Playlists, Queue, Songs, Radios, Settings, Similar. Similar has no `View` enum variant — it only renders inside the browsing panel.

## SlotListPageState

Shared by every slot-list view: search query, scroll position, focus index. Visible slot count is computed dynamically from window height (always odd, capped at `MAX_SLOT_COUNT = 29`); window resizes propagate via `update/window.rs`. Prefetch radius = `slot_count + MIN_PREFETCH_BUFFER` (3 by default).

**Multi-selection** (Ctrl+click / Shift+click / per-row checkbox / select-all): `selected_indices: HashSet`, `anchor_index` for range. `handle_slot_click()` handles modifier-aware selection; `handle_selection_toggle(offset, total)` and `handle_select_all_toggle(total)` drive the optional checkbox column. `select_all_state(total)` returns the tri-state (`None` / `Some` / `All`) the header bar uses. `clear_multi_selection()` resets. `evaluate_context_menu()` resolves batch targets for right-click menus.

**Auto-hide toolbar** (opt-in, Interface → Slot List, `settings.autohide_toolbar`): the view-header toolbar collapses to a thin strip until revealed. Reveal state lives on `SlotListPageState`: `toolbar_hovered` (cursor over the reveal zone), `toolbar_dropdown_open` (sort `pick_list` open), and `toolbar_reveal_until` (2.5 s hotkey reveal window set by `reveal_toolbar()`). `toolbar_revealed(autohide_enabled)` resolves these at render time — an active search query or focused search input also keeps it open. The `Toolbar*` `SlotListPageMessage` variants land via `set_toolbar_hovered()` / `set_toolbar_dropdown_open()` in both `SlotListPageState::handle()` and the `impl_expansion_update!` macro. **Invariant**: the flags are normally cleared by `on_exit` / `on_close`, which can't fire once the header unmounts — so `reset_reveal_locks()` must run on every unmount edge: view switch (`clear_all_toolbar_reveal_locks()` in `handle_switch_view`), browsing-panel close (`clear_browsing_panel_reveal_locks()`), and session reset. The collapse animation depends on the unconditional 100 ms `PlaybackMessage::Tick` for redraws — keep that tick ungated on playback.

## Navigation & Interaction

### Root-level SlotListMessage (keyboard + scrollbar)

`SlotListMessage` in `app_message.rs` carries global slot-list actions dispatched by hotkeys and scrollbar timers: `NavigateUp`, `NavigateDown`, `SetOffset(usize)`, `ActivateCenter`, `ActivateCenterShuffled` (Ctrl+Enter — force one-shot Shuffle Play of the centered item/selection), `ToggleSortOrder`, `ScrollbarFadeComplete(View, u64)`, `SeekSettled(View, u64)`. Root dispatch is in `handle_slot_list_message` (`update/slot_list.rs`). Each hotkey arm fans out to a per-view `Message::Albums(AlbumsMessage::SlotList(SlotListPageMessage::NavigateUp))` (and so on for every view), so the actual state mutation is always done by the per-view update handler.

### Per-view SlotList(SlotListPageMessage) carrier

Every per-view message enum carries a unified `SlotList(SlotListPageMessage)` variant (e.g., `AlbumsMessage::SlotList(…)`, `SongsMessage::SlotList(…)`). `SlotListPageMessage` (in `widgets/slot_list_page.rs`) enumerates all slot-list actions: `NavigateUp`, `NavigateDown`, `SetOffset(usize, Modifiers)`, `ScrollSeek(usize)`, `ActivateCenter(bool)` (`true` forces a one-shot Shuffle Play; `false` honors the `enter_shuffle` setting), `ClickPlay(usize)`, `SelectionToggle(usize)`, `SelectAllToggle`, `AddCenterToQueue`, `RefreshViewData`, `CenterOnPlaying`, `SearchQueryChanged(String)`, `SearchFocused(bool)`, `SortModeSelected(SortMode)`, `ToggleSortOrder`, `HoverEnterSlot(HoveredSlot)`, `HoverExitSlot(HoveredSlot)`, `ToolbarHoverEnter`, `ToolbarHoverExit`, `ToolbarDropdownToggled(bool)`. The slot-hover variants are published by per-slot `mouse_area::on_enter` / `on_exit` and land on `SlotListView::hovered_slot` (via `SlotHoverCallback` in `widgets/slot_list.rs`) so cross-pane drag resolves "cursor over which slot" structurally rather than from chrome math; `HoverExitSlot` is idempotent (only clears when its payload still matches). The three `Toolbar*` variants drive the auto-hide toolbar's reveal-lock state (see SlotListPageState above).

**Non-expansion views** (Songs, Queue, Radios, Similar) call `self.common.handle(msg, total)` → `SlotListPageAction`, then map the action to their `*Action` enum. `SlotListPageState::handle()` is the unified dispatcher.

**Expansion views** (Albums, Artists, Genres, Playlists) match `SlotList(msg)` sub-variants individually inside their update's `Err(msg)` arm (after `impl_expansion_update!` handles search/sort/expand variants), because navigation must route through expansion-aware methods like `expansion.handle_navigate_up()` / `expansion.handle_select_offset()`.

`dispatch_view_with_seek!` macro (in `update/mod.rs`) wraps each view's `handle_*` call: it detects if the message was a `SlotList(SlotListPageMessage::ScrollSeek(_))` and, if so, appends `scrollbar_fade_timer` + `seek_settled_timer` tasks.

- Non-wrapping navigation; dynamic center slot near edges.
- **Stable viewport** (default): non-center clicks → `SetOffset` (highlight in-place); center clicks → `ActivateCenter`.
- **Legacy mode**: non-center clicks → `ClickPlay` (direct play).
- Activation flash: `slot_list.flash_center()` on activation/transitions.
- Clickable text links: inline album/artist text dispatches `NavigateAndFilter { view, filter, for_browsing_pane }` via `mouse_area`. When the browsing panel is active, `for_browsing_pane: true` routes the change into its tab instead of switching the main view.
- Clickable star ratings + clickable hearts on every slot via `mouse_area`.
- Scrollbar timers carry the target `View` so seek messages route correctly between panes.

## Inline Expansion

Generic `ExpansionState<C>` + `SlotListEntry<P, C>`. When active, sort/search may target the expansion — check `expansion.is_expanded()`. Center-entry resolution is centralized in `views/expansion.rs`. Shift+Enter on Artists/Genres collapses the outer expansion.

**Find-and-expand** (clicking an inline album/artist/genre link or Shift+C from Songs): the chain runs through the `Nokkvi.pending_expand: state::PendingExpandState` cluster — `target: Option<PendingExpand>` (variants `Album { album_id, for_browsing_pane }`, `Artist { ... }`, `Genre { ... }`, `Song { song_id, for_browsing_pane }`) plus the `center_only` flag and the post-load `top_pin`. The `Song` variant exists only for the CenterOnPlaying fallback in the Songs view — clear search, paginate until the playing track appears, center on it without dispatching `FocusAndExpand`. Per-view `try_resolve_pending_expand_*` consume it once the target appears in its library buffer; `PendingTopPin` re-pins the highlight after the matching `set_children` lands. `for_browsing_pane = true` routes the final `FocusAndExpand` into the browsing-panel tab instead of the top pane. `PendingExpand::host_view()` drives the cancel-on-navigation check in `handle_switch_view`. The carrier messages are `NavigationMessage::Expand(PendingExpand)` (kick-off) and `NavigationMessage::ExpandTimeout(PendingExpand)` (2s "Finding {entity}…" toast) — both namespaced under `Message::Navigation` and dispatched via `update/navigation.rs`.

## Column Visibility (Albums / Artists / Genres / Playlists / Queue / Songs / Similar)

`widgets/checkbox_dropdown.rs` exposes a `checkbox_dropdown` (wrapped per view by `view_columns_dropdown`) of column toggles. The dropdown is a controlled overlay — opening it dispatches `Message::SetOpenMenu(Some(OpenMenu::CheckboxDropdown { view, trigger_bounds }))`. Similar lives only inside the browsing panel and lacks a `View::Similar` variant, so it uses its own `OpenMenu::CheckboxDropdownSimilar { trigger_bounds }`. Column flags persist on `PersistedPlayerSettings.view_columns` (the canonical `ViewColumns` struct in `data/src/types/view_columns.rs`, serde-flattened so the `{view}_show_*` keys — `_select`, `_index`, `_thumbnail`, `_album`, `_genre`, `_stars`, etc. — stay flat on the wire; `queue_show_default_playlist` is a header chip, not a column, and stays a direct field). Stars use responsive hide rather than per-mode toggling.

Each `define_view_columns!` entry has the form `Variant("Label"): field = default [=> setter @ settings_field]` — the macro emits the enum/struct/Default/get/set/toggle pieces plus `dropdown_entries()`, which builds the dropdown's `Vec<(Key, &'static str, bool)>` items. Declaration order == dropdown order and labels live in the declaration, so views pass `self.column_visibility.dropdown_entries()` to `view_columns_dropdown` / `similar_columns_dropdown` instead of hand-written `vec!`s. The Queue/Songs dropdown order (Genre right after Album) is pinned by `queue_and_songs_dropdown_order_is_pinned` in `update/tests/settings.rs`.

**Multi-select column**: opt-in `{view}_show_select` flag adds a per-row checkbox + tri-state "select all" header bar to every slot-list view. Helpers `wrap_with_select_column()` and `compose_header_with_select()` (`widgets/slot_list.rs`) keep per-view plumbing minimal; the checkbox state mirrors `selected_indices` regardless of how membership was set.

**Genre column** (Queue): stacks under the album when both columns are visible, takes over the album slot at album-size font when album is hidden. Auto-shows when sort = Genre (mirrors how the plays column auto-shows on MostPlayed sort).

## Context Menus & Toasts

- Library views: `LibraryContextEntry`. Queue: `QueueContextEntry`. Strip: `StripContextEntry`.
- Toast helpers: `toast_info()`, `toast_success()`, `toast_warn()`, `toast_error()`.
- Batch actions: context menu resolves targets via `evaluate_context_menu()` (or generates full-batch payloads for algorithmic views like Similar Songs), then dispatches batch operations. `clear_multi_selection()` after every batch completion.

## Browsing Panel (Split-View)

Toggled via Ctrl+E from Queue. `BrowsingView` enum: `Songs`, `Albums`, `Artists`, `Genres`, `Similar`. Reuses existing page structs. `PaneFocus` toggled via Tab.

**Cross-pane drag** state lives in the `Nokkvi.cross_pane_drag: state::CrossPaneDragUi` cluster (active drag + press tracking + pending drop position; manual `Default` keeps `selection_count` at 1). Batch support: `cross_pane_drag.selection_count` tracks single vs multi-selection batch. Drag threshold 5 px. Center index snapshotted at press time.

## Playlist Editing

`PlaylistEditState` for dirty detection. Inline name/comment editing lives in the dedicated `View::PlaylistEditor` view's edit-bar header (eyebrow + name input + comment input, stacked vertically). Save via `handle_save_playlist_edits()`. Browsing panel cannot close during edit.

## Queue Sort

Physical sort via `QueueManager::sort_queue()`, persists to redb. `QueueSortMode`: Album, Artist, Title, Duration, Genre, Rating, MostPlayed, Random (re-rolls on re-select / order toggle). Album column visible across all sort modes; stars use responsive hide. Sort signature is cached and `sort_by_cached_key` avoids re-keying when the signature is unchanged.

## Queue Shuffle

Re-shuffles the order array when a shuffled queue with repeat-playlist wraps back to the start, instead of replaying the same shuffle sequence.

## Roulette (slot-machine random pick)

Available on every slot-list view via the "Roulette" entry in the sort dropdown or the `Roulette` hotkey (default `Ctrl+R`). State on `Nokkvi.roulette: Option<state::RouletteState>` is snapshotted at start so live data churn (page loads, search edits, queue mutations) cannot drift the math. Two-phase: the cruise runs at constant velocity indefinitely until the user presses **Enter** (intercepted in `handle_slot_list_message` and dispatched as `RouletteMessage::Stop`), which rolls the landing target and arms `state.decel`. Tick handlers in `update/roulette.rs` derive the offset purely from elapsed time — cruise loops cyclically; decel walks the pre-rolled keyframe sequence (cubic-distributed holds + fake-out wobble) anchored at `stop_time`. Cancelled by Escape or view change; in-decel Enter is swallowed (the spin is committed once Stop fires).

## Update Handler Pattern

Root dispatch in `update/mod.rs`. `ls src/update/` for handler files. The async-bridge helpers `shell_task` / `shell_spawn` are methods on `Nokkvi` (`src/main.rs`). Cross-cutting helpers:

**`update/chrome.rs`** — shared handler prologue:
- `HasViewChrome` trait — implemented by all 8 library-view message types (Albums, Artists, Songs, Genres, Playlists, Queue, Radios, Similar). Classifies variants as `SetOpenMenu`, `Roulette`, nav-sfx, or expand-sfx.
- All 8 impls are generated by the file-private `impl_view_chrome!` macro. Each invocation declares the per-view variation axes as `{ roulette: yes|no, expand: yes|no, drag: yes|no }` (Similar has no `Roulette` variant; only the 4 expansion views flag expand SFX; Radios has no artwork-drag variants). A wrong flag is a compile error, not silent drift — `yes` references the variant by name. New view-message enums get one invocation plus axis flags, not a hand-written impl.
- `dispatch_view_chrome<M: HasViewChrome>(handler, msg, view)` — run at the top of every `handle_*` function. Returns `Some(task)` for `SetOpenMenu` / `Roulette` intercepts (caller returns immediately); returns `None` for normal page actions (after triggering the appropriate SFX).

**`update/components/`** (directory module: `mod.rs` + `artwork_prefetch.rs`) — shared action helpers:
- `guard_play_action` — pre-play hook that transitions radio playback back to queue mode (returns `None` to let the play proceed; retains an `Option` return so a future block condition could short-circuit a play — the former playlist-edit block was removed)
- `set_item_rating_task`, `star_item_task`, `radio_mutation_task`
- `handle_common_view_action` — applies generic Search/Sort/Navigate actions to non-Queue library views; called from each view's handler after the page `update()` returns a `CommonViewAction`
- `PaginatedFetch::from_common()` — needs_fetch-gated paginated load (Albums / Artists / Songs)
- `prefetch_album_artwork_tasks` / `prefetch_song_artwork_tasks` — viewport-window artwork prefetch; defined in `artwork_prefetch.rs`, re-exported so call sites keep using `components::<fn>`
- `play_entity_task` / `add_entity_to_queue_task` / `insert_entity_to_queue_at_position_task` — generic entity-action builders
- `reset_session_state(&mut self) -> Task<Message>` — full session-teardown reset (audio engine, task manager, queue/library/state/scrobble caches, focus, modals). Single source for logout + session-expired auth flows; callers add only their tail-specific work (toast, dialog) afterward.
- Boilerplate extraction helpers in `widgets/slot_list_page.rs` (`get_queue_target_indices`, `get_batch_target_indices`) and `views/expansion.rs` (`build_batch_payload`)

**`update/loader_target.rs`** — paged-loader unification (Group U Lane C):
- `LoaderTarget` trait per entity: `AlbumsTarget`, `ArtistsTarget`, `SongsTarget`, `GenresTarget`, `PlaylistsTarget`. Encapsulates the `page_common()` accessor and `sort_mode_to_api()` (plus library-buffer/artwork/viewport hooks).
- `Nokkvi::load_paged<T: LoaderTarget>(...)` owns the shared body (defensive `offset > 0 && needs_fetch().is_none()` gate, `set_loading(true)`, `shell_task` build). Page size is read from `settings.library_page_size` inside `load_paged`; the entity-specific paged-fetch closure is a per-call parameter captured at the call site, not threaded through the trait. Per-entity callers (`handle_load_*` / `handle_*_load_page` / `force_load_*_page`) shrink to a single `self.load_paged::<TargetType>(...)` line. New paged views implement `LoaderTarget` rather than copying the body.

## View Data Refresh

- **Manual**: header Refresh button or the `RefreshView` hotkey (default `R`) → `RefreshViewData` → the view's `reload_message()`, which busts the cache and refetches from source
- **Automatic**: Navidrome SSE → `update/library_refresh.rs` → ID-anchored background reload that preserves scroll position. The `background: true` flag on loaded messages prevents scroll jumps. Suppressed by `suppress_library_refresh_toasts`.

## Modals

- **Equalizer**: 10-band + presets (`widgets/eq_modal.rs`, `update/eq_modal.rs`). Selecting a preset auto-enables the EQ. Sliders visually reset to 0 dB when disabled.
- **About**: metadata/diagnostics, theme-adaptive logo (`widgets/about_modal.rs`, `update/about_modal.rs`). Includes a Ko-fi tip link. The Commit row hides gracefully when built outside a git context.
- **Info**: Get Info two-column property table (`widgets/info_modal.rs`, `update/info_modal.rs`). `InfoModalItem` variants per type.
- **Text Input**: name/comment edits + confirmations (`widgets/text_input_dialog.rs`).
- **Default Playlist Picker**: modal sub-slot-list to choose the default playlist (`widgets/default_playlist_picker.rs`, `update/default_playlist_picker.rs`). State on `Nokkvi.default_playlist_picker`; opened from the chip in the Playlists/Queue header or the Playback → Default Playlist settings entry.
- **Font / Theme pickers** (settings sub-lists): centered modal overlays over the settings panel, sharing all chrome (dimmed backdrop, title bar with X back-button, search bar) via `render_picker_modal()` in `views/settings/view.rs` (`render_font_modal` / `render_theme_modal`). State lives on `SettingsPage` as `font_sub_list: Option<FontSubListState>` and `theme_sub_list: Option<ThemeSubListState>` — mutually exclusive (only one picker opens at a time). The theme picker paints each row in its OWN theme's palette (hover wash disabled) so scrolling IS a live preview; the font picker draws each font name in its own typeface (per-row rendering via `render_theme_slot` / `render_font_slot` in `views/settings/rendering.rs`). Handled by the `update_font_sub_list` / `update_theme_sub_list` methods in `views/settings/sub_lists.rs` (dispatched from `SettingsPage::update` in `mod.rs`; no separate `update/` file). The non-modal `sub_list: Option<SubListState>` is the in-place color-array sub-list, not a picker overlay.

All wrapped in an overlay container with `mouse_area` for correct SVG rendering.

## System Tray

`src/services/tray.rs` runs a ksni-based StatusNotifierItem on a dedicated thread. `update/tray.rs` handles `TrayEvent` (toggle window, play/pause, next/prev, quit) and window-close-to-tray when `close_to_tray` is enabled.
