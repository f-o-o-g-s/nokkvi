---
trigger: model_decision
description: Common pitfalls and subtle bugs. Reference when debugging unexpected behavior or working with queue indices, optimistic UI, widget focus, audio locks, expansion sort state.
---

# Common Gotchas

## Queue & Indices
- **Filtered indices**: When search is active, slot list indices are relative to the **filtered** queue. Always map through `filtered_songs` for queue operations.
- **Queue peek/transition pattern**: All track transitions must use `peek_next_song()` → `transition_to_queued()`. Never set `current_index` directly for transitions. Use `set_current_index()` only for non-transition updates (play-from-here).
- **Progressive queue generation**: `ProgressiveQueueAppendPage` chains must check `progressive_queue_generation` before appending — stale chains silently stop.
- **Play button cold-start**: Uses `get_effective_center_index` (selected track), not `queue_songs.first()`.
- **Gapless re-peek on mutation**: If a queue mutation (add/remove) calls `clear_queued()` between gapless prep and `on_track_finished`, `transition_to_queued()` would return `None` → playback stalls. The navigator now re-peeks when `queued.is_none() && !needs_load` before transitioning.

## Optimistic UI & Race Conditions
- **Tick handler race**: The 10Hz tick can overwrite optimistic state with stale backend state. Use pending flags to prevent reversion before API response.
- **Source generation counter**: Renderer snapshots `source_generation` (AtomicU64) before releasing the engine lock; discards callback if it changed. Prevents consume+shuffle replaying the just-consumed track.
- **PagedBuffer pagination guard**: Call `set_loading(true)` before dispatching a page fetch — prevents duplicate fetches on rapid scroll.
- **LRU artwork snapshot staleness**: Call `refresh_large_artwork_snapshot()` after every `put()` or `get()` on the LRU cache.

## Widget Tree & Focus
- **Widget tree stability**: Changing root widget type (Row→Column) destroys `text_input` focus. Use `base_slot_list_empty_state` for consistent structure.
- **Search input ID collisions**: Each view needs a unique search input ID constant.
- **HoverOverlay must wrap a Container, not a Button**: Native `button` captures `ButtonPressed` before HoverOverlay's press tracker runs. Pattern: `mouse_area(HoverOverlay::new(container(content)...)).on_press(msg)`.
- **Length::Fill stripe in unconstrained Row**: `container(Space).height(Fill)` in a row without explicit height expands to fill all column space. Set `height(Shrink)` on the wrapper row.

## Audio Engine
- **Never hold engine lock during decoder operations.** Create fresh decoders on track change.
- **Crossfade trigger must be synchronous**: `render_tick`'s crossfade trigger must set `crossfade_active = true` synchronously before signaling the engine async. Otherwise EOF fires first → hard-cut.
- **Crossfade duration clamping**: `arm_crossfade()` clamps to `min(xfade, shorter_track / 2)` and skips for songs < 10s.
- **Stale gapless prep on mode toggles**: Mode toggle handlers must call `reset_next_track()` to clear prepared decoder and disarm crossfade trigger.
- **Pre-volume visualizer samples**: Visualizer receives raw samples before volume multiplication, scaled to S16 range. FFT input is volume-independent.
- **Visualizer buffer lifetime**: `pending_clear` atomic handles stale audio on track change — don't add extra clearing logic.

## Config & Persistence
- **Config writer routing**: `update_config_value()` → `config.toml`; `update_theme_value()` / `update_theme_color_array_entry()` → active theme file. Misrouting writes to the wrong file.
- **Config reload suppression**: `suppress_config_reload()` prevents file watcher feedback loops, but GUI-initiated theme/visualizer changes need manual `ThemeConfigReloaded` trigger after write.
- **Font propagation**: `font_family` is in the theme file. Changes must trigger `ThemeConfigReloaded`. EQ modal `pick_list` must explicitly receive the theme font.

## Artwork
- **Handle::from_bytes for refresh**: `Handle::from_path` uses file path as ID → stale GPU texture on overwrite. Use `Handle::from_bytes(data)` for content-derived IDs.
- **Queue song mini vs large artwork**: Queue songs request 80px thumbnails. Large artwork fallback must construct full-size URL (`size=1000`), not reuse the 80px thumbnail URL.

## Misc
- **CenterOnPlaying (Shift+C)**: Must directly call `handle_set_offset()`, not dispatch `SlotListMessage::SetOffset` (which routes through click-to-highlight path).
- **Expansion sort state**: When expansion is active, sort/search may target the expansion. Check `expansion.is_expanded()`.
- **Playlist edit guard**: Use `guard_play_action()` at the top of every play handler.
- **Chrome height**: Must account for all visible header bars. Update constants in `widgets/slot_list.rs` when chrome changes.
- **Cross-pane drag center index**: Snapshotted on drag activation (5px threshold) — decoupled from subsequent state changes.
- **Database lock on re-login**: Cache `StateStorage` via `AppService::new_with_storage()`, stop engine + `TaskManager` on logout.
- **Stale progress track segments**: When metadata toggle changes, overlay_segments must be rebuilt and a `Tick` dispatched to force re-render.
