---
trigger: model_decision
description: Common pitfalls, race conditions, and subtle bugs. Reference when debugging unexpected behavior or working with queue indices, optimistic UI, widget focus, audio locks, expansion sort state.
---

# Common Gotchas

## 1. Queue Filtered Indices

When search is active, slot list indices are relative to the **filtered** queue, not the full queue. Always map through `filtered_songs` when translating a slot list index to a queue position. This applies to **all** queue operations: context menu actions, play next, remove, reorder.

## 2. Optimistic UI + Tick Handler Race

The 10Hz tick can overwrite optimistic state with stale backend state. Use pending flags to prevent the tick from reverting changes before the API response arrives.

## 3. Widget Tree Stability

Changing the root widget type (e.g., `Row` → `Column`) between renders **destroys `text_input` focus**. Use `base_slot_list_empty_state` to maintain consistent widget tree structure.

## 4. Audio Engine Lock Contention

Never hold the engine lock during decoder operations — this causes deadlocks. Create fresh decoders on track change.

## 5. Expansion Sort State

When a view has inline expansion active, sort/search operations may target the **expansion** rather than the parent list. Always check `expansion.is_expanded()`.

## 6. Search Input ID Collisions

Each view has a unique search input ID constant. Without unique IDs, switching views can transfer focus to the wrong search field.

## 7. Visualizer Buffer Lifetime

When tracks change, the visualizer sample buffer may contain stale audio. The `pending_clear` atomic flag handles this — don't add additional clearing logic.

## 8. Database Lock on Re-Login

Cache `StateStorage` across logout/login (via `AppService::new_with_storage()`) and stop the audio engine + `TaskManager` on logout to prevent "Database already open" errors.

## 9. CenterOnPlaying vs Stable Viewport

`CenterOnPlaying` (Shift+C) **must** directly call `handle_set_offset()`, not dispatch `SlotListMessage::SetOffset`. The latter routes through `handle_select_offset` (click-to-highlight) which only highlights without scrolling.

## 10. Stable Viewport Auto-Follow

Even with stable viewport enabled, the viewport auto-follows on **natural** track transitions (not user-initiated play actions).

## 11. PagedBuffer Pagination Guard

Always call `set_loading(true)` **before** dispatching a page fetch. Without this guard, rapid scrolling triggers duplicate fetches.

## 12. LRU Artwork Snapshot Staleness

Call `refresh_large_artwork_snapshot()` after every `put()` or `get()` on the LRU cache, otherwise `view()` renders stale data.

## 13. Progressive Queue Generation Counter

`ProgressiveQueueAppendPage` chains must check `progressive_queue_generation` before appending — stale chains silently stop.

## 14. Playlist Edit Guard

Use `guard_play_action()` from `update/components.rs` at the top of every play handler. Returns `Some(Task)` with a toast warning if in edit mode.

## 15. Browsing Panel Lazy Data Load

Tab switches check `is_empty()` on `LibraryData` and dispatch `LoadX` if needed. Forgetting causes empty tabs.

## 16. Drop Indicator Positioning

Drop indicator line must appear **between** slots. `compute_queue_drop_slot()` translates cursor Y → queue index accounting for viewport offset and chrome height.

## 17. Cross-Pane Drag Center Index Snapshot

When a drag activates (exceeds 5px threshold), the browsing view's `effective_center_index` is **snapshotted** — decoupled from subsequent state changes.

## 18. Queue Insert Position vs Append

Cross-pane drag drops check `pending_queue_insert_position` — if `Some(index)`, insert at position; if `None`, append.

## 19. Chrome Height with Headers

Chrome height calculations must account for all visible header bars (nav bar, edit bar, playlist header, browsing tab bar). Update constants in `widgets/slot_list.rs` when chrome changes.

## 20. Source Generation Counter (Consume+Shuffle Race)

The renderer's track-completion callback must check `source_generation` before processing. The engine increments on every `set_source()`; the renderer snapshots it and discards the callback if it changed. Prevents consume mode from removing the wrong track.

## 21. Crossfade Trigger Must Be Synchronous

`render_tick`'s crossfade trigger must start the crossfade stream **synchronously** (set `crossfade_active = true`) before signaling the engine async. Otherwise the EOF completion check fires before the engine responds — hard-cut instead of crossfade.

## 22. Crossfade Current Index Sync

On gapless/crossfade transitions, `current_index` must be synced to the actually-playing track, not the track that was prepared. Race: async crossfade setup can lag behind consume-mode index updates.

## 23. Pre-Volume Visualizer Samples

The visualizer receives **pre-volume** samples from `StreamingSource` — the raw sample is fed to `viz_buffer` before volume multiplication. This ensures the FFT input is volume-independent, matching the old PipeWire behavior where volume was applied at the stream level.

## 24. Queue Peek/Transition Pattern

All track transitions (gapless, crossfade, manual skip) must use `peek_next_song()` → `transition_to_queued()`. Do NOT set `current_index` directly for transitions — that desynchronizes `current_order` and `queued`. Use `set_current_index()` for non-transition index updates (e.g., play-from-here).

## 25. Crossfade Duration Clamping

`arm_crossfade()` clamps the effective duration to `min(xfade, shorter_track / 2)` and skips crossfade entirely for songs < 10s. Don't assume the crossfade duration equals the user's configured value — always use the armed/effective duration from the renderer.

## 26. Artwork Refresh Must Use Handle::from_bytes

`Handle::from_path` derives its ID from the file path. During a refresh, the disk cache file is overwritten at the same path — so `from_path` produces the same Handle ID, and Iced's GPU texture cache serves the stale texture. The refresh handler (`handle_refresh_album_artwork`) must use `Handle::from_bytes(data)` so the Handle ID is content-derived, busting the stale cache entry.

## 27. Stale Gapless Prep on Mode Toggles

Toggling shuffle/repeat/consume after gapless preparation (~80% through track) leaves a stale decoder in the engine. Mode toggle handlers must call `reset_next_track()` to clear the prepared decoder and disarm the crossfade trigger, plus reset `gapless_preparing` in the UI so prep re-triggers with the correct next track.

## 28. Stale Struct Fields in Visualizer Builder

The `Visualizer` struct caches config values (e.g., `border_width`) set at construction time. Builder methods like `width()` that re-read some fields from shared config must re-read **all** config fields they use — partial re-reads cause layout/render divergence. The `view()` method reads fresh config for the shader, so if `width()` uses a stale field the layout won't match what the GPU renders.

## 29. Artwork Size: Queue Song Mini vs Large

Queue songs request 80px thumbnails for slot list mini artwork. When the large artwork pipeline falls back to loading from the network (cache miss), it must construct a **full-size** cover art URL — not reuse the 80px thumbnail URL from the song's `cover_art` field. The queue fallback in `handle_load_large_artwork` must build the URL with `size=1000` (or omit size) for crisp large artwork.

## 30. HoverOverlay Must Wrap a Container, Not a Button

Native `button` captures `ButtonPressed` before `HoverOverlay`'s passive event tracker can run — the press state is never set, the scale animation never fires. Pattern: `mouse_area(HoverOverlay::new(container(content)...)).on_press(msg)`. Never: `HoverOverlay::new(button(content).on_press(msg))`.

## 31. Length::Fill Stripe in Unconstrained Row in Column

A `container(Space).height(Length::Fill)` inside a `row![]` that itself has no explicit height will cause the row to expand to fill all remaining column space — not just match sibling element heights. Always set `height(Length::Shrink)` on the wrapper row when adding a right-edge stripe to a header element whose height is determined by its main content (e.g., view_header at 48px). The stripe itself should still use `Fill` so it spans the row's allocated height.
