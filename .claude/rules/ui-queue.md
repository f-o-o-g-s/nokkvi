---
paths:
  - "src/views/{queue,playlist_editor}/**"
  - "src/update/{queue,progressive_queue,editor,rules_editor,nsp_import,cross_pane_drag,harbour,ipc}.rs"
  - "src/update/hotkeys/{mod,queue}.rs"
  - "src/state/{playlist_editor,rules_session,autoplay}.rs"
  - "src/update/tests/{queue,editor,rules_editor,autoplay}.rs"
  - "src/update/tests_queue_filter.rs"
---

# Queue & Playlist Editor

## Queue Sort

Physical sort via `QueueManager::sort_queue()`, persists to redb; `apply_queue_sort` also sorts the UI's rows at once (`sort_queue_songs`), and the view takes the backend's rows at its next reload. Both sorts and `queue_is_sorted` key through `QueueSortMode::sort_key` (`data/src/types/queue_sort_mode.rs`, fields read through `QueueSortFields`), built once per row (`sort_by_cached_key`); gotchas/queue.md "A queue sort runs twice". Random re-rolls on re-select / order toggle and is dispatched separately. Album column visible across all sort modes; stars use responsive hide. An unchanged `(mode, ascending, len)` signature skips the UI sort.

**"Unsorted" placeholder**: the queue takes its order from whatever populated it (play album, session restore, add/remove, drag, consume, SSE refresh), so `QueuePage.queue_sort_mode` is only the *remembered* mode. The sort dropdown shows a grayed "Unsorted" placeholder (`sort_placeholder` on the view header) until `QueuePage.queue_sorted` is true. Only `apply_queue_sort` promotes it; `revalidate_queue_sorted` (`src/main.rs`) demotes — never promotes — by re-verifying the live order against the applied mode (`queue_is_sorted`) and clearing the sort-signature cache. It runs from `handle_queue_loaded` and after every in-place reorder (drag, batch drag, Shift+arrow move).

## Queue Drag Reorder

Drags are blocked (and half-captured pick state dropped) while a search filter is active; a past-end / empty-area drop appends (`slot_to_item_index_for_drop(...).unwrap_or(total_items)`).
entry_id rules: see gotchas/queue.md.

**Pointer-motion fast paths**: `ScrollSeek` returns from its own earlier fast path (an O(n) filter under a search); the per-cursor-move `HoverEnterSlot` / `HoverExitSlot`, `DragReorder(Dragged)` and `LyricsWheel` return from `handle_queue`'s pointer-motion block before the row list is read. The block lists every other `QueueMessage` variant instead of `_`, so a new message fails to compile until it is placed, and a per-cursor-move one joins the block. Harbour, whose handler rebuilds every row, and the playlist editor, whose handler re-plans the viewport prefetch and the centered row's large art, return the slot-hover and toolbar-reveal messages first; the exhaustive `SlotListPageMessage::is_hover_or_reveal` sorts them. Their `ScrollSeek` keeps the full path (Harbour clamps against the built row count; the editor arms no seek-settled timer, so its seek pass loads the art). So no hover pass covers an editor buffer change: remove, drag-drop and Remove Duplicates each return the prefetch themselves (rows slide into view, the centred song changes). `DragColumn` publishes `Dragged` on every `CursorMoved`, so per-event work proportional to the queue backs up the event loop (a ~20k-row queue froze the window). Thumbnails for rows the edge auto-scroll brings in come from `tick_within_list_autoscroll`, which returns the viewport prefetch whenever it moved the viewport, for the queue and the playlist editor alike.

## Queue Server Sync

The Queue header carries a folded server-sync action menu (`OpenMenu::QueueSync`) with push / pull, backed by `push_queue_task` / `pull_queue_task` (`update/queue.rs`) and also reachable via the `queue-push` / `queue-pull` CLI verbs. Shown when `supports_index_based_queue()` (the server advertises the OpenSubsonic indexBasedQueue extension) and radio is inactive; the IPC verbs check both through `guard_queue_sync`. Pull cues without auto-playing — see backend-services.md / audio-engine.md.

## Playlist Editing

`PlaylistEditState` for dirty detection. `View::PlaylistEditor` hosts two session kinds (`EditorSessionKind::{Tracks, Rules}`, `src/state/playlist_editor.rs`):
- **Tracks** — the ordered-track editor. Edit-bar header: eyebrow + name input + comment input (left), public/lock toggle + remove-duplicates + save + discard (right). Save via `handle_save_playlist_edits()`. Browsing panel cannot close during edit. Remove Duplicates (the edit-bar button and the row menu entry under Remove from Playlist, both `EditorMessage::RemoveDuplicates`) filters the WHOLE buffer through `nokkvi_data::utils::dedupe::duplicate_entry_ids` (first copy of each song id stays; an active search, the clicked row and the multi-selection never narrow it) and is staged like every edit until Save.
- **Rules** — the smart-playlist rules editor (`views/playlist_editor/rules_view.rs`; handlers `update/rules_editor.rs` + `.nsp` import `update/nsp_import.rs`; root msgs `Message::RulesEditor(RulesEditorMessage)` + `NspImportPicked`). The header import uploads without opening the editor, so it runs the editor's `refresh_delay_diagnostic` on the file first: an Error refuses the file, a Warning (server older than 0.64) toasts and imports. Two-pane `[results/preview | rules form]` layout with nested rule groups, chip cells, and an inline calendar date-picker overlay; no browsing panel. The preview pane owns its own columns dropdown (`OpenMenu::CheckboxDropdownPreview` — no `View` variant, parallel to Similar). Reached via `PlaylistContextEntry::EditRules` and the Playlists-header create menu (`OpenMenu::PlaylistsCreate`: New Playlist / New Smart Playlist / Import .nsp…). Editing-mode keys route through `rules_session_key_intercept` (see gotchas/hotkeys.md "Bare modifier/lock keydowns are dropped"). **Gate placement**: `handle_raw_key_event` runs it only with no root modal open and outside theater, and before any resolved action dispatches, so no SFX or toolbar arm fires first. A free-text `Refresh` row (per-playlist refresh delay) sits under `Limit` and renders when `caps.per_playlist_refresh_delay` or the rules already carry a delay; preview-draft bodies go through `SmartRules::to_preview_value` (via `RulesSessionUi::draft_preview_body`), which strips the delay, while Save keeps `to_value`.

## Autoplay (UI)

The mode's backend is backend-services.md "Autoplay"; the UI owns when to ask (`update/playback.rs`, state `Nokkvi.autoplay: state::AutoplayState`).

- **Level-triggered from the tick**: `PlaybackStateUpdate` carries `autoplay_runway` (`QueueManager::runway`, read under the tick's queue lock) and `queue_ran_out_epoch`; `autoplay_tick` runs at the end of every `handle_playback_state_updated`, and the pure `AutoplayState::ask(&AutoplayGate, now)` decides (on, queue mode, Repeat off, playing or paused, runway at most the watermark, no fill in flight, no progressive load (`queue_loading_total()`), the retry gate open). No queue mutation hooks it.
- **Outcomes** (`handle_autoplay_filled`, `now` a parameter): `Added` reloads the queue, no toast; `NothingFound` and a failure count toward `AUTOPLAY_MAX_RETRIES` and wait `AUTOPLAY_RETRY_SECS` (`NothingFound` toasts once per song); `NotNeeded` waits 2 s, counts nothing; a song change resets all three. A rejected session goes to `SessionExpired`.
- **Run-out resume**: the first ran-out count of a session is only adopted (the backend restarts it at 0 per login); a move with the mode on, queue mode and Repeat off sets `resume_pending` and asks `autoplay_top_up(true)` at once unless a fill is in flight. The next `Added` plays its `first_entry_id` through `resume_into_autoplay` (no `guard_play_action`: queue mode only, nothing to leave). A playing tick, `handle_stop` / `handle_play` / `handle_toggle_play`, `guard_play_action`, mode off and logout drop it (`forget_autoplay_resume`).
- **The row mark**: rows with `QueueSongUIViewData::autoplay` lead their artist line with the infinity glyph (`AUTOPLAY_ROW_MARK` through `slot_list_text_column_marked`, `views/song_list_pane.rs`); the playlist editor's rows always carry `false`. A row with an empty artist line shows no mark (the column drops the subtitle).
- **Going off is one function**, `autoplay_went_off(persist)`: the toggle (bar, kebab, `a`, `nokkvi autoplay`) and the Settings row (`finish_general_setting`, keyed by `settings_row_turns_autoplay_off`) both call it; it resets the state (keeping a fill in flight) and runs `clear_pending_autoplay`, whose count supersedes the "Autoplay: Off" toast (`ToastState::supersede`). A batch landing after the off clears again. A config.toml hand edit does not run it.

