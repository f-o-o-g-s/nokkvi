---
trigger: model_decision
description: Common pitfalls and subtle bugs. Reference when debugging unexpected behavior or working with queue indices, optimistic UI, widget focus, audio locks, expansion sort state.
---

# Common Gotchas

## Queue & Indices

- **Filtered indices**: when search is active, slot-list indices are relative to `filtered_songs` — always map through the filtered view before queue mutations.
- **Queue remove uses song IDs, not indices**: `QueueAction::RemoveFromQueue` / `PlayNext` carry `Vec<String>` of song IDs. `track_number` cannot be used to map filtered display index → backend queue index because it drifts after any optimistic in-place mutation or client-side sort. Convert at the view boundary; everything downstream goes through `QueueManager::remove_song_by_id` / `remove_songs_by_ids`.
- **Stale multi-selection across refreshes**: `handle_queue_loaded` and `apply_queue_sort` clear `selected_indices` + `anchor_index`. Without this, indices kept pointing at whatever rows occupied those positions after a consume-mode auto-advance / external refresh — different songs.
- **Three sources of truth for "what's playing"**: `QueueManager.current_index`, `QueueNavigator.current_song_id`, and the engine's active source. The remove path uses `decide_removal_aftermath` (pure) → `PlaybackController::apply_removal_aftermath` to keep all three in sync; never bypass it. History append is intentionally skipped on remove (the song was deleted, not skipped past).
- **Queue peek/transition**: track transitions go `peek_next_song()` → `transition_to_queued()`. Use `set_current_index()` ONLY for non-transition updates (play-from-here).
- **Progressive queue generation**: `ProgressiveQueueAppendPage` chains check `progressive_queue_generation` before appending — stale chains silently stop.
- **Play button cold-start**: uses `get_effective_center_index` (selected track), not `queue_songs.first()`.
- **Gapless re-peek on mutation**: a queue mutation between gapless prep and `on_track_finished` calls `clear_queued()`, so `transition_to_queued()` would return `None`. The navigator re-peeks when `queued.is_none() && !needs_load` before transitioning.

## Multi-Selection & Batch

- **Ctrl+click toggle**: deselecting the last item must clear `selected_offset` to remove the center-highlight from the deselected slot.
- **Shift+click range**: clears existing selection first, then adds the range from `anchor_index` to clicked offset.
- **Context menu batch**: `evaluate_context_menu()` checks if the clicked index is in the selection; if not, resets selection to just that item.
- **Always `clear_multi_selection()` after batch ops** — prevents stale selections.
- **Cross-pane drag batch**: `cross_pane_drag_selection_count` is snapshotted at press time; decoupled from subsequent selection changes.
- **Keyboard scroll clears selection**: `handle_navigate_up/down` clears `selected_offset` to prevent stale highlights.

## Optimistic UI & Race Conditions

- **Tick handler race**: 10 Hz tick can overwrite optimistic state with stale backend state — use pending flags to prevent reversion before API response.
- **Source generation counter**: renderer snapshots `source_generation` (AtomicU64) before releasing the engine lock and discards the callback if it changed. Prevents consume+shuffle replaying the just-consumed track.
- **PagedBuffer pagination guard**: call `set_loading(true)` before dispatching a page fetch — prevents duplicate fetches on rapid scroll. `PaginatedFetch::from_common()` handles this in update handlers.
- **PagedBuffer generation**: `generation()` bumps on every mutation. Use `(query, generation)` keys when memoizing filtered results.
- **LRU artwork snapshot staleness**: call `refresh_album_art_snapshot()` after every `put()` / `get()` on `album_art`, and `refresh_large_artwork_snapshot()` for `large_artwork`. Forget either and the next render shows stale thumbnails.

## Widget Tree & Focus

- **Widget tree stability**: changing the root widget type (Row→Column) destroys `text_input` focus. Use `base_slot_list_empty_state` for consistent structure.
- **Search input ID collisions**: each view needs a unique search input ID constant.
- **HoverOverlay wraps containers, not native buttons** — buttons capture `ButtonPressed` early. Pattern: `mouse_area(HoverOverlay::new(container(content))).on_press(msg)`.
- **`Length::Fill` stripe in unconstrained Row**: `container(Space).height(Fill)` in a row without explicit height expands to fill column space. Set `height(Shrink)` on the wrapper row.
- **Single-active overlay menu**: hamburger / kebab / checkbox-dropdown / context menus must NOT own local `is_open` state. Bubble `Message::SetOpenMenu(Some(OpenMenu::…))` to root — opening a new one atomically replaces the previous one.

## Audio Engine

- **Decoder operations**: create fresh decoders and release the engine lock beforehand on track changes.
- **Crossfade trigger must be synchronous**: `render_tick`'s crossfade trigger sets `crossfade_active = true` synchronously before signaling the engine async — otherwise EOF fires first → hard cut.
- **Crossfade duration clamping**: `arm_crossfade()` clamps to `min(xfade, shorter / 2)` and skips for songs < 10 s.
- **Stale gapless prep on mode toggles**: mode toggle handlers call `reset_next_track()` to clear the prepared decoder and disarm the crossfade trigger.
- **Pre-volume visualizer samples**: visualizer receives raw samples before volume multiplication, scaled to S16 range. FFT input is volume-independent.
- **Track-completion lock**: the navigator releases its lock across engine I/O during track completion — do not re-introduce a held lock.
- **ReplayGain stash**: incoming-track ReplayGain must be stashed via `set_pending_replay_gain()` / `set_pending_crossfade_replay_gain()` before stream creation; the engine pulls the right factor at primary or crossfade-stream creation time.
- **Repeat track replay**: `on_track_finished` natively supports repeat-track via seek-to-start. Manual skip (next/prev) bypasses repeat-track.

## Config & Persistence

- **Typed config writer routing**: `ConfigKey::AppScalar` / `AppArrayEntry` → `config.toml`; `ConfigKey::Theme` / `ThemeArrayEntry` → active theme file. Match on the variant — never sniff key prefixes.
- **Config reload suppression**: `suppress_config_reload()` blocks the file watcher, but GUI-initiated theme/visualizer writes need a manual `ThemeConfigReloaded` trigger after the write.
- **Font is global, not per-theme**: `font_family` lives in `PlayerSettings` / `TomlSettings` and routes to `config.toml`. EQ modal `pick_list` must explicitly receive the active app font.
- **Database lock on re-login**: redb holds an exclusive lock; cache `StateStorage` on `Nokkvi.cached_storage` and reuse via `AppService::new_with_storage()`. Stop the engine + `TaskManager` on logout.

## Assets & Icons

- **Auto-generated SVG lookup**: `assets/icons/*.svg` is enumerated at build time by `build.rs`, generating `OUT_DIR/embedded_svg_generated.rs`. Adding/removing an icon is just dropping the file. Unknown paths still silently fall back to `play.svg` with a warn log — the test `all_svg_paths_in_source_are_registered` (`cargo test --bin nokkvi -- embedded_svg`) catches typos in path strings.

## Artwork

- **No client-side persistent cache**: every artwork fetch goes straight to Navidrome via `AlbumsService::fetch_album_artwork(...)`. Session-scoped Handle reuse comes from the UI's `album_art` (LRU 512) and `large_artwork` (LRU 200) maps in `ArtworkState`.
- **Always `Handle::from_bytes`**: `from_bytes` allocates a fresh `Id::Unique` per call — safe **only** because Handles are stored in the LRUs and reused across renders. Never re-create from bytes per frame; that bypasses Iced's GPU texture cache (`reference-iced/wgpu/src/image/raster.rs:55`).
- **Snapshot mirrors**: `view()` borrows `album_art_snapshot` and `large_artwork_snapshot` (HashMap mirrors), not the LRUs directly, because LRU `get` is `&mut`. Every `put` MUST be followed by the matching `refresh_*_snapshot()`.
- **Queue mini vs large artwork**: queue songs request 80 px thumbs using `album_id` so `fetch_album_artwork` builds a consistent URL across consumers. Large artwork constructs the full-size URL (`size=artwork_resolution`) — never reuse the 80 px URL.
- **Wildcard SSE skips artwork**: `LibraryChanged { is_wildcard: true }` (full-library scan) MUST NOT trigger silent re-fetch — it would re-download every cached cover. Slot-list reloads still run.
- **Random-sort SSE protection**: background SSE reload mustn't corrupt the artwork ref when the active sort is Random — guarded in `library_refresh.rs`.
- **Albums viewport clamp**: clamp `viewport_offset` against the new total count on background refresh — otherwise the viewport can land past the end after a remove.

## Misc

- **CenterOnPlaying (Shift+C)**: call `handle_set_offset()` directly. Dispatching `SlotListMessage::SetOffset` routes through the click-to-highlight path.
- **Expansion sort state**: when expansion is active, sort/search may target the expansion. Check `expansion.is_expanded()`. Artists/Genres are 2-tier — Shift+Enter on a centered child album row routes through `NavigateAndExpandAlbum` to the Albums view, not an inline 3rd-tier drill.
- **Pending find-and-expand chain**: at most one `Nokkvi.pending_expand` runs at a time. Starting a new chain (or any user-driven view change matching `PendingExpand::host_view()`) supersedes the previous one. `PendingTopPin` re-pins the highlight after `set_children` lands.
- **Expansion artwork retry**: artwork fetches dispatched from inline expansions retry on transient failure and reject empty-bytes responses, so a flaky first request doesn't leave a permanent empty cell.
- **Playlist edit guard**: `guard_play_action()` at the top of every play handler.
- **Chrome height**: must account for every visible header bar. Update constants in `widgets/slot_list.rs` when chrome changes.
- **Cross-pane drag center index**: snapshotted on drag activation (5 px threshold) — decoupled from subsequent state changes.
- **Stale progress-track segments**: when a metadata toggle changes, `overlay_segments` must be rebuilt and a `Tick` dispatched to force re-render.
- **Workspace lints are deny-level ship blockers**: `unwrap_used`, `print_stdout`, `print_stderr`, `dbg_macro`, `mem_forget`, `todo`, `unimplemented`, `or_fun_call`, `unused_async`, `match_wildcard_for_single_variants` all `deny` in `[workspace.lints.clippy]`. Tests opt out via `#![cfg_attr(test, allow(...))]` at each crate root; intentional CLI prints use targeted `#[allow]`. Don't paper over with broader allows — fix at the call site.
