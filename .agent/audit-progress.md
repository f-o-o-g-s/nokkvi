# Audit progress tracker

Tracks completion status of the 2026-05-07 DRY/scalability/monolith audit at `~/nokkvi-audit-results/_SYNTHESIS.md`. The audit was generated against `main @ bc53b17` (2026-05-07).

**Read this before starting any audit-derived task.** Pick up where the last commit left off, not where the source report said things stood.

Last verified: **2026-05-08** (full §7 + §5 + spot-checks across §3/§4/§6).

---

## How to use this doc

- Items marked **✅ done** ship with the commit refs that closed them. Use those as patterns when the same shape repeats.
- Items marked **🟡 partial** have foundational infrastructure but the final replacement step is incomplete. The note tells you what remains.
- Items marked **❌ open** were verified open at the date above. Re-verify with a quick grep before declaring you'll work on them — code may have moved.
- Items marked **❓ stale** mean the audit's path / line / pattern no longer matches code; locate the actual current location before starting.

When you complete an item, append the commit ref(s) and flip the status. Keep the table short — the source reports already have the full justification.

---

## §7 action queue (12 ranked items)

| Rank | Item | Status | Evidence |
|---:|---|---|---|
| 1 | Bug fixes batch (B1, B2, B3, B6, B8, B9) | ❌ open | None of the 6 bugs in the batch have closed. See §5 table below for per-bug detail. |
| 2 | `View::ALL` + replace 8 wildcard `_ =>` arms | ❌ open | No `View::ALL` declared in `src/main.rs`. `src/update/navigation.rs` now has 10 `_ =>` arms (audit said 8 — count grew). |
| 3 | Extend `define_view_columns!` to emit `persist_*_column_visibility` | ❌ open | 7 hand-written `persist_*_column_visibility` functions still live in `src/update/`. |
| 4 | Migrate `hotkeys/star_rating.rs` to `star_item_task`/`set_item_rating_task` | ❓ stale path | `src/hotkeys/star_rating.rs` does not exist. Only `global.rs` and `mod.rs` live in `src/hotkeys/`. The inline-rebuild pattern (if still present) is somewhere else; locate before starting. |
| 5 | `enum ItemKind` to replace `item_type: &str` | ❌ open | 7 sites still pass `item_type: &str` (e.g. `src/update/components.rs:783-806`). No `enum ItemKind` defined. |
| 6 | `update/navigation.rs` pending-expand dedup + paired tests/navigation.rs macro | ❌ open | `src/update/navigation.rs` is still 1134 LOC with 4 hand-written pending-expand functions. |
| 7 | AppService `LibraryOrchestrator` + `QueueOrchestrator` split | ❌ open | 4 `play_X` + 4 `add_X_to_queue` + matching `play_next_X` / `insert_X_at_position` still hand-written on `AppService`. No `LibraryOrchestrator` / `QueueOrchestrator` types. No `enum SongSource`. |
| 8 | Loader-result `LoaderTarget` trait | ❌ open | The `*LoaderMessage` Phase 1+2 scaffolding landed pre-audit (commits `171c053..bc53b17`) but the unifying `LoaderTarget` trait was not introduced. The 5 `handle_*_loaded` bodies are still parallel. |
| 9 | Move slot_list + roulette per-`View` dispatch onto `ViewPage` trait | 🟡 partial | `pub(crate) trait ViewPage` exists at `src/views/mod.rs:55` with rich API (search_input_id, sort_mode_options, toggle_sort_order_message, etc.). BUT `src/update/slot_list.rs:115-158` still has 8 `View::X` match arms; the migration onto the trait is not done. |
| **10** | **`define_settings!` macro** | **✅ done** | Pre-audit: `48d022c` (scaffold) → `46d4717` (General) → `4d268b3` (Interface) → `8e13d81` (Playback + Theme-top) → `db5faf1` (drop helper) → `eb94d56` (sync UI cache). Post-audit follow-up fanout (2026-05-08): `e7c6314` (lane D — sync-setter watchpoint), `3e70230..969f9f8` (lane A — read-side mirror, 3 commits), `3f254e1..4f4e728` (lane B — legacy-arm fold, 3 commits), `7176b6c` (cleanup — `get_player_settings` to `..Default::default()`), `a23eeb2` (artwork-resolution toast text fix), `2f5a484..5e73346` (lane C — items-builder driver, 7 commits). The strangler-fig is fully retired; `define_settings!` now emits dispatch / apply / dump / items-helper artifacts; `ui_meta:` cluster is the discriminator for UI-emitting vs lifecycle-only entries; `entries.rs:193` search filter widened to match `item.subtitle`. |
| 11 | `ConfigKey` typed-key constructors (drop `is_theme_path` runtime classifier) | ❌ open | `is_theme_path` is still active in `src/config_writer.rs:55`; both `for_value` (line 37) and `for_array` (line 47) sniff the prefix at runtime. |
| **12** | **Type-level queue invariants (IG-1 + IG-2 + IG-4 + IG-5)** | **✅ done** | Three-lane fanout from `.agent/plans/queue-typestate-igs.md` (2026-05-08). Lane A (IG-1 + IG-2) `9a8fa7c`: `get_queue_mut` deleted; `insert_song_at` renamed to `insert_song_and_make_current` + body delegated through `insert_songs_at`. Lane B (IG-4) `1e42e13..61ec876`: `PeekedQueue<'a>` borrow-guard at `data/src/services/queue/navigation.rs`; `peek_next_song` returns `Option<PeekedQueue<'_>>`; `transition_to_queued` narrowed to `pub(crate) transition_to_queued_internal`; Drop runs `clear_queued` so peek-without-transition is a clean reset. Lane C (IG-5 + queue-side IG-3) `1d2d9c8..4e2c960`: `QueueWriteGuard` in `data/src/services/queue/write_guard.rs`; every mutator in `mod.rs` runs through `let mut tx = self.write(); …; tx.commit_save_{all,order,no_save}()`; `clear_queued` call sites in `mod.rs` drop from 11 to 0; `set_repeat`/`toggle_consume` now clear `queued` (locked by `set_repeat_clears_queued`/`toggle_consume_clears_queued`). |

---

## §5 bugs (B1–B11)

| Bug | Location (per audit) | Status | Note |
|---|---|---|---|
| B1 | `src/widgets/nav_bar.rs:327`, `side_nav_bar.rs:252`, `views/login.rs:301` — `HoverOverlay::new(button(...))` | ❌ open | Still 3 sites at the audit-cited lines. The `button(...)` argument starts on the line *after* `HoverOverlay::new(`, so single-line greps for `HoverOverlay::new(button(` miss it. |
| B2 | `src/views/login.rs:226,253,282` + `widgets/info_modal.rs:559,565` — `radius: 4.0.into()` | ✅ done | `e48b809` (2026-05-08): all 5 sites switched to `theme::ui_border_radius()`. Login card radius (`login.rs:343`, `12.0`) intentionally unchanged. |
| B3 | `src/views/queue/view.rs` — queue header morphs widget-tree depth across edit/playlist-context/read-only modes | ✅ done | Lane C `d16694c` (2026-05-08): every branch now produces the same `column![extra, sep, header]` shape; read-only branch uses zero-sized `Space::new()` placeholders (Shrink × `Length::Fixed(0.0)`) so visual output is unchanged but the search `text_input::Id` parent stays positionally stable across edit-mode toggles. |
| B4 | `src/update/tests/general.rs::toggle_light_mode_persists_to_settings_key` — mutates env vars + reads disk | ❌ open | Test still present. |
| B5 | `src/update/tests/settings.rs::settings_general_*_artwork_overlay_flips_theme_cache` family — asserts on process-global atomics | ❓ unverified | Grep returned 0 matches for the named tests. They may have been renamed, removed, or live elsewhere. Locate before declaring. |
| B6 | `src/widgets/hamburger_menu.rs:401-407` — `match item_index { 0 => …, 4 => Quit }` paired with `MENU_ITEM_COUNT = 5` const | ❌ open | `MENU_ITEM_COUNT` referenced 3× (line 315, 321, 456); the indexed `match item_index` still at line 401. |
| B7 | `src/update/settings.rs:373,391` — `visualizer.waves` ↔ `visualizer.monstercat=0.0` mutual-exclusion does not call `reload_visualizer_config()` after the secondary write | ❌ open | The secondary-write block calls `patch_cached_entry` but does not dispatch `reload_visualizer_config` after. Live `Arc<RwLock<VisualizerConfig>>` and audio engine still hold the old monstercat value until next user-driven write. |
| B8 | `src/update/tests/navigation.rs:1043` — test `albums_loaded_re_pins_selected_offset_for_artist` body operates on Artists | ✅ done | Lane E `c514d68` (2026-05-08): renamed to `artists_albums_loaded_re_pins_selected_offset_in_artists_view` to match the test body's Artists-view focus. |
| B9 | `src/update/mod.rs:230-238` — comment claims `*LoaderMessage` migration is partial, but Phase 2 is complete | ✅ done | Lane E `c514d68` (2026-05-08): replaced the 9-line block with a 3-line accurate description; the "stubs (`unimplemented!()`) until Phase 2" claim (false since `31374ec..bc53b17`) is gone. Sibling `Note:` comments at `:250`, `:260`, `:268` still describe past completed migrations and remain accurate. |
| B10 | `src/update/hotkeys/star_rating.rs` (genres.rs:354-389, playlists.rs:231-253, artists.rs:524-568) — sub-fetch Err arms return `NoOp` instead of `SessionExpired` | ❓ unverified | The `src/hotkeys/star_rating.rs` path doesn't exist; `update/genres.rs`/`playlists.rs`/`artists.rs` are the actual locations. Locate the Err arms and grep for `SessionExpired` / `handle_session_expired` before declaring. |
| B11 | `data/src/audio/engine.rs:235-240` — `live_icy_metadata.try_write()` vs `live_codec_name.write()` asymmetry | ✅ done | Lane A `418ce27` (2026-05-08): `live_codec_name` reset in `set_source` switched to `try_write()` to match `live_icy_metadata`. The two other `live_codec_name.write()` sites (decoder-init L356, gapless transition L1199) run inside the engine lock where contention is impossible — left unchanged. |

---

## §6 type-level invariant gaps (IG-1 through IG-14)

| ID | Gap | Status | Evidence |
|---|---|---|---|
| IG-1 | `QueueManager::get_queue_mut() -> &mut Queue` raw escape hatch | ✅ done | Lane A `9a8fa7c` (2026-05-08): deleted; verified zero callers outside the definition before removal. |
| IG-2 | `insert_song_at` (singular, sets current_index) vs `insert_songs_at` (plural, doesn't) — opposite playhead semantics under near-identical names | ✅ done | Lane A `9a8fa7c` (2026-05-08): renamed to `insert_song_and_make_current`; body refactored to delegate through `insert_songs_at` + `set_current_index` so the playhead-jumping semantics are explicit rather than mirrored in a parallel body. |
| IG-3 | Mode-toggle methods (`toggle_shuffle`, `set_repeat`, `toggle_consume`) don't compel `engine.reset_next_track()` | ✅ done | Queue-side half closed in queue-typestate-igs Lane C `4e2c960` (2026-05-08): all three mode toggles now go through `QueueWriteGuard` and clear `queued` consistently. Engine-side closed in audio-engine-typestate-igs Lane C `be4659f` + `4a90597` (2026-05-08): `types/mode_toggle.rs::ModeToggleEffect` is a `#[must_use]` token returned by `QueueManager::toggle_shuffle`/`set_repeat`/`toggle_consume`; its only consumer is `effect.apply_to(&engine).await`, so a future caller cannot toggle a queue mode and silently skip the gapless-prep reset. `PlaybackController` site count for `engine.reset_next_track()`: 3 → 0. |
| IG-4 | `peek_next_song` → `transition_to_queued` discipline is doc-only | ✅ done | Lane B `1e42e13..61ec876` (2026-05-08): `PeekedQueue<'a>` borrow-guard owns the only public commit path; Drop runs `clear_queued`; `transition_to_queued` narrowed to `pub(crate) transition_to_queued_internal`. |
| IG-5 | `clear_queued()` after every queue mutation enforced by 11+ explicit call sites | ✅ done | Lane C `1d2d9c8..4e2c960` (2026-05-08): `QueueWriteGuard` in `data/src/services/queue/write_guard.rs`; every mutator in `mod.rs` runs through `let mut tx = self.write(); …; tx.commit_save_*()`; Drop is the safety net for `?` / panic paths; `clear_queued` call sites in `mod.rs`: 11 → 0. |
| IG-6 | `decode_generation: Arc<AtomicU64>` free-floating, 6 `fetch_add(1)` sites | ✅ done | Lane A `d7f92f9` (2026-05-08): `DecodeLoopHandle` newtype in `data/src/audio/generation.rs`; every "stop the decode loop" path now goes through `supersede() -> u64`; spawned-loop equality check uses `current()`. Raw `fetch_add` sites for `decode_generation`: 6 → 0. |
| IG-7 | `source_generation` semantics — increment-or-not is doc-only per site | ✅ done | Lane A `d7f92f9` (2026-05-08): `SourceGeneration` newtype with named verbs (`bump_for_user_action`, `bump_for_gapless`, `accept_internal_swap` no-op). Crossfade-finalize comment is now an actual call. Raw `fetch_add` sites for `source_generation`: 2 → 0. |
| IG-8 | `CrossfadePhase` transitions enforced by 5 mutation sites; nothing prevents `OutgoingFinished` directly from `Idle` | ✅ done | Lane B `c1f4676` + `5099e76` + `02dafa5` (2026-05-08): renderer-side `CrossfadeState` enum-with-data (Idle / Armed / Active{stream,…}) replaces 9 parallel fields; engine-side `CrossfadePhase::{Idle, Active{decoder,incoming_source}, OutgoingFinished{decoder,incoming_source}}` carries the per-phase data so transitions are one `mem::replace` and `OutgoingFinished` can only be reached by destructuring `Active`. New `tests::crossfade_idle_cannot_transition_directly_to_outgoing_finished` pins the runtime behavior. |
| IG-9 | `set_current_index` doc-only "play-from-here only" contract | ❌ open | `pub fn set_current_index` still at `data/src/services/queue/mod.rs:411`; not renamed. |
| IG-10 | `pub` shared atomics on `AudioRenderer` (engine, source_generation, decoder_eof) — anyone with `&mut AudioRenderer` can rotate them | ✅ done | Lane A `3a74372` (2026-05-08): all three fields are now private; `AudioRenderer::set_engine_link(engine, source_generation, decoder_eof)` is the sole installation path; `engine.set_engine_reference` calls it. |
| IG-11 | RG-stash + `set_source` / `load_track` + `play()` sequencing — 4 hand-paired sites | ✅ done | audio-engine-typestate-igs Lane C `9d9cefa` + `acd5d31` (2026-05-08): added `CustomAudioEngine::load_track_with_rg(url, rg)` (engine.rs:1084) that pairs the renderer RG-stash and the source-update atomically; migrated all seven hand-paired sites (5 in `playback_controller.rs`, 2 in `services/playback.rs`). `set_pending_replay_gain` site count in `playback_controller.rs` + `services/playback.rs`: 7 → 0. The crossfade-side `set_pending_crossfade_replay_gain` stays public — it's the next-track slot, distinct from primary-stream RG-stash. |
| IG-12 | `TaskManager::spawn` / `spawn_result` ignore `shutdown()`; only `spawn_cancellable` observes the token | ❌ open | No `spawn_detached` introduced. Three spawn variants still hand-written with different cancellation semantics. |
| IG-13 | Gapless lock-acquisition order across 3 tokio mutexes (`next_source_shared`, `decoder`, `next_track_prepared`) | ✅ done | Lane D `cf6f00f` + `69306e2` (2026-05-08): the three engine-internal tokio mutexes (`next_decoder`, `next_track_prepared`, `next_source_shared`) collapsed into one `Arc<tokio::sync::Mutex<GaplessSlot>>` on `CustomAudioEngine`. All decode-loop, engine async, `start_crossfade`, `load_prepared_track`, `reset_next_track`, and `is_next_track_prepared` sites now take the same mutex once and operate on `slot.{decoder, source, prepared}` together — the lock-order question disappears. Lock acquisitions in `engine.rs`: ~25 → 11. `GaplessSlot::is_prepared() = prepared && decoder.is_some()` invariant pinned by 5 unit tests. |
| IG-14 | `take_*_receiver` single-shot enforced by `Option::take` (silent `None` for second caller) | ❌ open | Not verified. Re-check before declaring. |

---

## §3 DRY findings (1–20) — selective verification

§7 #1, #3, #4, #6, #7 already cover DRY items 1, 3, 6, 4, 1 respectively. The remaining items are smaller wins. Spot-checks below; consult `~/nokkvi-audit-results/dry-*.md` for the unverified items.

| # | Item | Status | Evidence |
|---:|---|---|---|
| 1 | Pending-expand × {Album, Artist, Genre, Song} dedup | ❌ open | Same as §7 #6. |
| 2 | `handle_*_loaded` LoaderTarget trait | ❌ open | Same as §7 #8. |
| 3 | Per-view column-visibility persisters | ❌ open | Same as §7 #3. |
| 4 | AppService entity × verb matrix | ❌ open | Same as §7 #7. |
| 5 | Settings 3-parallel-list drift | ✅ done | Same as §7 #10. |
| 6 | Hotkey star/rating boilerplate | ❓ stale path | Same as §7 #4. |
| 7 | Per-row library context-menu wrapper | ❌ open | No `wrap_library_row` helper in `src/widgets/`. |
| 8 | Per-view "columns cog" dropdown | ❌ open | Not verified. Re-check before declaring. |
| 9 | Paginated library loader Pattern A | ❌ open | No `paginated_load_task` helper. |
| 10 | Bulk fixture + scenario-seeder helpers in tests | ❌ open | Not verified. |
| 11 | Handler prologue (SetOpenMenu / Roulette / play_view_sfx) | ❌ open | No `dispatch_view_chrome` free fn. |
| 12 | `AddBatchToQueue` insert-or-append | ❌ open | No `add_or_insert_batch_to_queue_task` helper. |
| 13 | `ToggleStar` with optimistic revert | ❌ open | No `toggle_star_with_revert_task` helper. |
| 14 | 3D-button pressed-state color ramp | ❌ open | No `BevelStateColors::compute()` in `src/widgets/`. |
| 15 | Sub-fetch Unauthorized routing | ❓ unverified | Same as B10 — locate first. |
| 16 | `HasCommonAction` opt-out for Radios | ❌ open | Not verified. Re-check before declaring. |
| 17 | Stream URL building 5× | 🟡 partial | `fn build_stream_url` exists in 2 spots in `data/src/`; whether the 5 historical sites all route through it is not verified. |
| 18 | AppService `_api()` factories | ❌ open | No `api_factory!` macro. The 5 factory methods are still hand-written. |
| 19 | Direct callers of `update_config_value` / `update_theme_value` | ❌ open | Not verified. |
| 20 | EQ + SFX text-toggle in player bar | ❌ open | Not verified. |

---

## §4 drift findings (1–14) — selective verification

§7 #2, #5, #11 cover Drift 1, 2, 8. Other items below.

| # | Item | Status | Evidence |
|---:|---|---|---|
| 1 | `View` enum match-block fanout + 8 silent `_ =>` arms | ❌ open | Same as §7 #2. |
| 2 | `item_type: &str` carrying entity kind | ❌ open | Same as §7 #5. |
| 3 | Settings 3 parallel lists | ✅ done | Same as §7 #10. |
| 4 | `HotkeyAction` parallel matches (`hotkey_action_to_message`, `hotkey_action_to_key`) | ❌ open | Not verified. The hotkey macro consolidation in `da1723d` (pre-audit) closed part of this; the two parallel matches the audit cites may still be hand-written. |
| 5 | Visualizer parallel `Vec<f64>` arrays | ❌ open | Not verified. |
| 6 | `SortMode` × per-view `*_OPTIONS` arrays | ❌ open | No central `pub const TABLE` in `data/src/types/sort_mode.rs`. |
| 7 | `OpenMenu::CheckboxDropdown { view: View::X, ... }` per-view construction | ❌ open | No `SlotListPageState::checkbox_dropdown_open_message` helper. |
| 8 | `update_config_value` vs `update_theme_value` runtime classifier | ❌ open | Same as §7 #11. |
| 9 | Per-view message enums + bubble-only intercepts | ❌ open | Not verified. |
| 10 | Hardcoded `Some(80)` instead of `Some(THUMBNAIL_SIZE)` | ❌ open | 4 sites still: `data/src/backend/albums.rs:67`, `src/update/window.rs:159`, `src/update/songs.rs:255`, `src/update/components.rs:164`. |
| 11 | Hamburger menu match-arms | ❌ open | Same as B6. |
| 12 | Visualizer config dotted keys (37 distinct literals) | ❌ open | No typed visualizer-key enum on `ConfigKey`. |
| 13 | Crossfade armed/active dual-flag in `AudioRenderer` | ✅ done | Lane B `c1f4676` (2026-05-08): folded into `CrossfadeState` enum (see IG-8). The `crossfade_active` / `crossfade_armed` bools and the 3 `crossfade_armed_*` fields are gone — `is_crossfade_active()` / `is_crossfade_armed()` now `matches!` the variant. |
| 14 | Missing `View::ALL` / `NavView::ALL` declarations | ❌ open | Same as §7 #2. |

---

## Quick-pick: highest-leverage open items

If picking the next item to work, these are the highest agent-friendliness payoff per the audit's ranking and remain open:

1. **§7 #2 — `View::ALL` + replace 8 wildcards** (S effort, foundational, every future View change benefits).
2. **§7 #3 — `define_view_columns!` persist emission** (M effort, the most-frequent feature edit; persist-arm omission fails silently on relaunch).
3. **§7 #5 — `enum ItemKind`** (M effort, kills `_ => Song` silent-default class outright).
4. **Bugs B1, B2, B6, B8, B9** (S effort each, real visible bugs and a stale comment that misleads future agents).

§7 #6, #7, #12 are L effort; not first picks unless explicitly scheduled. (§7 #10 was the third L-effort item; it landed across the 2026-05-08 follow-up fanout.)
