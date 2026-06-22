---
trigger: model_decision
description: Common pitfalls and subtle bugs. Reference when debugging unexpected behavior or working with queue indices, optimistic UI, widget focus, audio locks, expansion sort state.
---

# Common Gotchas

## Queue & Indices

- **Filtered indices**: when search is active, slot-list indices are relative to `filtered_songs` — always map through the filtered view before queue mutations.
- **Queue rows are addressed by `entry_id` (per-row `u64`), not indices, not `track_number`**: `track_number` is a 1-based queue-position label stamped at backend-projection time (`build_queue_song_ui_view_data` in `data/src/backend/queue.rs`). It drifts the moment the UI applies an optimistic local mutation, and stays stale until the backend re-projection arrives. Every UI write path against the queue carries `entry_id`(s) and bottoms out at: `play_entry_from_queue`, `move_queue_batch_by_entry_ids`, `remove_entries_by_ids`, `play_next_in_queue`. The Focus path mirrors: `FocusCurrentPlaying` / `FocusOnSong` carry `u64` and the handler does `position(|s| s.entry_id == eid)` — never compare `track_number`. `MoveItem` (single-row drag) still uses raw indices because its dispatch is search-guarded; if a future caller plumbs MoveItem from a filtered context, migrate it to `entry_id` like `MoveBatch`.
- **Stale multi-selection across refreshes**: `handle_queue_loaded` and `apply_queue_sort` clear `selected_indices` + `anchor_index`. Without this, indices kept pointing at whatever rows occupied those positions after a consume-mode auto-advance / external refresh — different songs.
- **Three sources of truth for "what's playing"**: `QueueManager.current_index`, `QueueNavigator.current_song_id`, and the engine's active source. The remove path uses `decide_removal_aftermath` (pure) → `PlaybackController::apply_removal_aftermath` to keep all three in sync; never bypass it. History append is intentionally skipped on remove (the song was deleted, not skipped past).
- **Queue peek/transition**: track transitions go `peek_next_song()` → `PeekedQueue::transition()`. Use `reposition_to_index()` ONLY for non-transition updates (play-from-here).
- **Progressive queue generation**: `ProgressiveQueueAppendPage` chains check `progressive_queue_generation` before appending — stale chains silently stop.
- **Play button cold-start**: resolves the selected row via `get_center_item_index` and plays its `entry_id` via `play_entry_from_queue` — not `queue_songs.first()`, not `track_number - 1`.
- **Gapless re-peek on mutation**: a queue mutation between gapless prep and `on_track_finished` calls `clear_queued()`. The transition path always re-peeks — `peek_next_song()` re-computes `queued` when it was cleared, then `PeekedQueue::transition()` consumes it — so the mutation can't strand the transition.

## Multi-Selection & Batch

- **Ctrl+click toggle**: deselecting the last item must clear `selected_offset` to remove the center-highlight from the deselected slot.
- **Shift+click range**: clears existing selection first, then adds the range from `anchor_index` to clicked offset.
- **Context menu batch**: `evaluate_context_menu()` checks if the clicked index is in the selection; if not, resets selection to just that item.
- **Always `clear_multi_selection()` after batch ops** — prevents stale selections.
- **Cross-pane drag batch**: `cross_pane_drag.selection_count` (on the `CrossPaneDragUi` cluster) is snapshotted at press time; decoupled from subsequent selection changes.
- **Keyboard scroll clears selection**: `handle_navigate_up/down` clears `selected_offset` to prevent stale highlights.

## Optimistic UI & Race Conditions

- **Tick handler race**: 10 Hz tick can overwrite optimistic state with stale backend state — use pending flags to prevent reversion before API response.
- **Source generation counter**: typed `SourceGeneration` wrapper (over `AtomicU64`) — every user-driven source change goes through `bump_for_user_action()`; renderer snapshots `current()` before releasing the engine lock and discards the callback if it changed. Prevents consume+shuffle replaying the just-consumed track.
- **PagedBuffer pagination guard**: call `set_loading(true)` before dispatching a page fetch — prevents duplicate fetches on rapid scroll. `PaginatedFetch::from_common()` handles this in update handlers.
- **PagedBuffer generation**: `generation()` bumps on every mutation. Use `(query, generation)` keys when memoizing filtered results.
- **Artwork LRU caches go through `SnapshottedLru<K, V>`**: `album_art`, `large_artwork`, and the `{mini, collage}` pair on each `CollageArtworkCache` (two instances — `ArtworkState.genre` and `ArtworkState.playlist`) are `SnapshottedLru` newtypes that maintain the view-borrowable `HashMap` snapshot automatically. Never pair a bare `lru::LruCache` with a manual `HashMap` snapshot — a fresh cache must use `SnapshottedLru`.

## Widget Tree & Focus

- **Widget tree stability**: changing the root widget type (Row→Column) destroys `text_input` focus. Use `base_slot_list_empty_state` for consistent structure.
- **Search input ID collisions**: each view needs a unique search input ID constant.
- **HoverOverlay wraps containers, not native buttons** — buttons capture `ButtonPressed` early. Pattern: `mouse_area(HoverOverlay::new(container(content))).on_press(msg)`.
- **`Length::Fill` stripe in unconstrained Row**: `container(Space).height(Fill)` in a row without explicit height expands to fill column space. Set `height(Shrink)` on the wrapper row.
- **Single-active overlay menu**: hamburger / kebab / checkbox-dropdown / context menus must NOT own local `is_open` state. Bubble `Message::SetOpenMenu(Some(OpenMenu::…))` to root — opening a new one atomically replaces the previous one.

## Audio Engine

- **Decoder operations**: create fresh decoders and release the engine lock beforehand on track changes.
- **Crossfade trigger must be synchronous**: `render_tick`'s crossfade trigger swaps `crossfade_state` from `Armed` to `Active` via `mem::replace` synchronously before signaling the engine async — otherwise EOF fires first → hard cut.
- **Crossfade duration clamping**: `arm_crossfade()` clamps to `min(xfade, shorter / 2)` and skips for songs < 10 s (`MIN_CROSSFADE_TRACK_MS`).
- **Bit-perfect gates the crossfade**: `arm_crossfade()` returns early when `crossfade_blocked(current_format, incoming_format)` — Strict hard-cuts every transition; Relaxed hard-cuts only on a cross-format change. The engine's EOF-fallback transition gates on the SAME `(current, incoming)` pair, so neither trigger can start a blend the other refuses and orphan the incoming stream.
- **Stale gapless prep on mode toggles**: mode toggle handlers call `reset_next_track()` to clear the prepared decoder and disarm the crossfade trigger. `set_bit_perfect()` shares this contract — on a REAL mode change (the renderer reports the flip) it calls `reset_next_track()` too, because bit-perfect flips crossfade eligibility and an armed transition could otherwise desync. It's a no-op when unchanged so a routine settings save can't disturb an in-flight transition.
- **Pre-volume visualizer samples**: visualizer receives raw samples before volume multiplication, scaled to S16 range. FFT input is volume-independent.
- **Track-completion lock**: the navigator releases its lock across engine I/O during track completion — do not re-introduce a held lock.
- **ReplayGain stash**: prefer `engine.load_track_with_rg(url, rg, expected_duration_ms)` — the atomic pair that stashes ReplayGain on the renderer and then calls `set_source(url, expected_duration_ms)` so a load can't be interleaved. Use `set_pending_crossfade_replay_gain()` for the crossfade decoder before its stream is built.
- **Repeat track replay**: `on_track_finished` natively supports repeat-track by re-loading the same row — it returns `TrackTransitionPlan::LoadFresh` (reload from the stream URL) or `PlayPrepared` (the gapless-prepared decoder), not a seek-to-start. Manual skip (next/prev) bypasses repeat-track (`get_next_song` in `services/queue/navigation.rs`).

## Config & Persistence

- **Typed config writer routing**: `ConfigKey::AppScalar` → `config.toml`; `ConfigKey::Theme` / `ThemeArrayEntry` → active theme file. Match on the variant — never sniff key prefixes.
- **Config reload suppression**: the file watcher suppresses its own reflections via an identity-based registry — `record_internal_write()` stamps each write's `(path, content-hash)` and `was_internal_write()` (`data/src/utils/paths.rs`) matches it within a monotonic 500ms window — but GUI-initiated theme/visualizer writes still need a manual `ThemeConfigReloaded` trigger after the write.
- **Font is global, not per-theme**: `font_family` lives in `LivePlayerSettings` / `TomlSettings` and routes to `config.toml`. EQ modal `pick_list` must explicitly receive the active app font.
- **Database lock on re-login**: redb holds an exclusive lock; cache `StateStorage` on `Nokkvi.cached_storage` and reuse via `AppService::new_with_storage()`. Stop the engine + `TaskManager` on logout.

## Assets & Icons

- **Auto-generated SVG lookup**: BOTH icon namespaces — `assets/icons/*.svg` (Lucide, the paths every view references) and `assets/icons-phosphor/*.svg` (the alternate set) — are enumerated at build time by `build.rs`, generating `OUT_DIR/embedded_svg_generated.rs` (one `lookup()` keyed by full path + `KNOWN_PATHS`). Adding/removing an icon is just dropping the file in either dir.
- **Selectable icon set remaps in `get_svg`** (`src/embedded_svg.rs`): when the active set is Phosphor (the **default**), `get_svg(path)` first runs `phosphor_path()` (binary-searches the sorted `NAME_MAP` Lucide-stem → Phosphor-file table) and returns the mapped Phosphor bytes. A mapped-but-MISSING Phosphor file falls through **gracefully to the Lucide content** (not the play.svg fallback). The Lucide set skips the remap entirely. Only a genuinely **unknown** path hits the silent `play.svg` fallback (with a warn log). The filled transport/rating glyphs (`play`, `pause`, `skip-back`, `skip-forward`, `heart-filled`, `star-filled`) deliberately map to the Phosphor **Fill** weight; the rest use Regular.
- **`cargo test --bin nokkvi -- embedded_svg`** runs the guard suite: `all_svg_paths_in_source_are_registered` catches typo'd path strings; `generated_paths_match_assets_dir` re-confirms codegen saw both dirs; the `icon_name_map_*` tests pin the `NAME_MAP` (sorted, covers every Lucide icon, every Phosphor target ships, Fill weight for the filled glyphs); `get_svg_honors_active_icon_set` pins the remap end-to-end.

## Artwork

- **No client-side persistent cache**: every artwork fetch goes straight to Navidrome via `AlbumsService::fetch_album_artwork(...)`. Session-scoped Handle reuse comes from the UI's `album_art` (LRU 1024 — doubled when the 2×2 quads landed, since a quad row claims up to 4 ids) and `large_artwork` (LRU 200) maps in `ArtworkState`.
- **Version-aware refetch + negative cache**: the prefetch gates run through `should_refetch(cached_ids, versions, failed, id, updated_at)`. `ArtworkState.album_art_versions` (album-id → the `updated_at` cache-buster that warmed each slot) lets album-coherent surfaces (Albums view, Artists/Genres expansion — they pass `album.updated_at`) re-fetch when the server cover changes. Passive surfaces (queue, song-mini, similar, playlist editor) carry only a per-song `updated_at` that would oscillate the album-id-keyed map, so they feed a constant `None` via `passive_artwork_version()` (id-only dedup). `ArtworkState.failed_art` is the negative cache: album/artist ids whose 80px fetch returned NO image (code-70 "Artwork not found" — an album that merely lacks a cover gets a placeholder *image* and caches normally, so it never lands here). Membership stops re-queuing a known-dead id on every scroll/resize/view-switch; a CHANGED `updated_at` bypasses the entry and re-attempts. Both maps are unbounded but reset wholesale by `ArtworkState::default()` on logout, so server-A failures never suppress server-B art.
- **Always `Handle::from_bytes`**: `from_bytes` allocates a fresh `Id::Unique` per call — safe **only** because Handles are stored in the LRUs and reused across renders. Never re-create from bytes per frame; that bypasses Iced's GPU texture cache (`reference-iced/wgpu/src/image/raster.rs:55`).
- **Snapshot mirrors**: `view()` borrows the `HashMap` snapshot inside each `SnapshottedLru`, not the LRU directly, because LRU `get` is `&mut`. The newtype keeps both in sync on every `put` / `get_touch` promotion; no caller-side discipline needed.
- **Queue mini vs large artwork**: queue songs request 80 px thumbs using `album_id` so `fetch_album_artwork` builds a consistent URL across consumers. Large artwork constructs the full-size URL (`size=artwork_resolution`) — never reuse the 80 px URL.
- **Quad fetches gate on `album_art_pending`**: the 2×2 quad prefetch is re-dispatched from every scroll step and collage event, so each ×4 quad path filters against `ArtworkState.album_art_pending` AND inserts its queued ids (`quad_prefetch_tasks` / `strip_quad_prefetch_tasks` in `src/update/collage.rs`); `handle_artwork_loaded` releases the slot on success and failure alike so a throttled tile can retry. The expansion album fan-out (`expansion_album_artwork_tasks`) also filters on the set — genre expansion children lead with the same albums their row quad is warming, and `FocusAndExpand` fires both in one event cluster. Single-id prefetch surfaces stay gate-free; any new ×4 quad path must both consult and insert.
- **Frozen strip quad identity**: the queue strip's playlist quad renders from `strip_quad_album_ids`, snapshotted via `snapshot_strip_quad_ids()` on the FIRST queue load for the active playlist context (`handle_queue_loaded`) — at that moment queue order == playlist track order. Later queue reloads (consume advance, sort, SSE) intentionally leave the frozen ids untouched so prefetch and render agree on identity; `clear_active_playlist()` drops the snapshot with the context so the next playlist re-freezes from its own queue head.
- **Wildcard SSE skips artwork**: `LibraryChanged { is_wildcard: true }` (full-library scan) MUST NOT trigger silent re-fetch — it would re-download every cached cover. Slot-list reloads still run.
- **Random-sort SSE protection**: background SSE reload mustn't corrupt the artwork ref when the active sort is Random — guarded in `library_refresh.rs`.
- **Albums viewport clamp**: clamp `viewport_offset` against the new total count on background refresh — otherwise the viewport can land past the end after a remove.

## Misc

- **MPRIS multi-instance bus name**: nokkvi suffixes its bus name with `instance{pid}` (per the MPRIS spec) so two running instances don't silently fight over `org.mpris.MediaPlayer2.nokkvi` — without the suffix the loser of the race ends up with no MPRIS at all and nothing logs it. Don't drop the suffix.
- **CenterOnPlaying (Shift+C)**: call `handle_set_offset()` directly. Dispatching `SlotListMessage::SetOffset` routes through the click-to-highlight path.
- **Global hotkeys are suppressed on the login screen**: `handle_raw_key_event` early-returns when `screen == Screen::Login`. The login form owns its own keyboard (Tab focus + Enter on_submit); without the guard the always-on keyboard subscription double-dispatches — Tab is whitelisted past the capture guard and resolves to `SlotList(NavigateDown)` against the off-screen queue (firing the Tab SFX twice, churning hidden selection), and bare `x`/`c` toggle random/consume against an idle engine. Leave the `event::listen_with` subscription intact (reverting it reintroduces the swallowed Escape/Enter regression).
- **Expansion sort state**: when expansion is active, sort/search may target the expansion. Check `expansion.is_expanded()`. Shift+Enter on Artists/Genres collapses the outer expansion.
- **Pending find-and-expand chain**: at most one `Nokkvi.pending_expand.target` (on the `PendingExpandState` cluster) runs at a time. Starting a new chain (or any user-driven view change matching `PendingExpand::host_view()`) supersedes the previous one. `PendingTopPin` re-pins the highlight after `set_children` lands.
- **Expansion artwork retry**: artwork fetches dispatched from inline expansions retry on transient failure and reject empty-bytes responses, so a flaky first request doesn't leave a permanent empty cell.
- **Playlist edit guard**: `guard_play_action()` at the top of every play handler.
- **Chrome height**: must account for every visible header bar. Update constants in `widgets/slot_list.rs` when chrome changes.
- **Cross-pane drag center index**: snapshotted on drag activation (5 px threshold) — decoupled from subsequent state changes.
- **Mode-gated mini-player artwork**: `Nokkvi::mini_player_artwork()` returns `None` when `TrackInfoDisplay != MiniPlayer` — every other strip mode hides the mini-player section, so resolving the cached handle would be wasted work. Tests pin the gate in `update/tests/redesign_chrome.rs`.
- **Workspace lints are deny-level ship blockers**: `unwrap_used`, `print_stdout`, `print_stderr`, `dbg_macro`, `mem_forget`, `todo`, `unimplemented`, `or_fun_call`, `unused_async`, `match_wildcard_for_single_variants`, `assertions_on_constants` all `deny` in `[workspace.lints.clippy]`. Tests opt out via `#![cfg_attr(test, allow(...))]` at each crate root; intentional CLI prints use targeted `#[allow]`. Don't paper over with broader allows — fix at the call site. `assertions_on_constants` pushes load-bearing constant invariants into `const _: () = assert!(…)` blocks (compile-time) instead of runtime `assert!(<const expr>)` calls the optimizer eats.
