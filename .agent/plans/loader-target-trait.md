# `LoaderTarget` trait — fanout plan (§7 #8 / DRY #2)

Closes `.agent/audit-progress.md` §7 #8 and §3 #2. Collapses the 5-way parallel in `handle_*_loaded` (Albums / Artists / Songs / Genres / Playlists) and the 3-way parallel in `handle_*_page_loaded` (Albums / Artists / Songs) into one generic body each, driven by a `LoaderTarget` trait with per-entity impls. Fixes two silent bugs in the process. Consolidates `force_load_songs_page` duplication.

Last verified baseline: **2026-05-09, `main @ HEAD`** (lane-A starts from current main; lanes B and C start after lane A merges).

Source reports: `~/nokkvi-audit-results/{_SYNTHESIS.md, dry-handlers.md, monoliths-ui.md}`.

---

## 1. Goal & rubric

The five `handle_*_loaded` bodies share a 9-step skeleton:

```
count_mut = total_count
set_first_page(items)
apply_viewport_on_load (background / anchor branch)
post_load_ok_hook (playlists picker refresh)
prefetch_artwork_tasks
center_large_artwork_task
try_resolve_pending_expand
--- Err branch ---
if Unauthorized → set_loading(false) + handle_session_expired
else → error log + set_loading(false) + [cancel_pending_expand] + toast_error
```

They already diverge in ways that cause silent bugs:

| Divergence | Albums | Artists | Songs | Genres | Playlists |
|---|---|---|---|---|---|
| anchor-miss fallback | clamp to `new_len−1` | reset to 0 | reset to 0 | ❌ no anchor | ❌ no anchor |
| `selected_indices.retain` on background reload | ✅ | ❌ missing | ❌ missing | — | — |
| `cancel_pending_expand` on Err | ✅ | ✅ | **❌ BUG** | ✅ | ❌ (ok, no expand) |

The generic body bakes in the correct behaviour everywhere: `selected_indices.retain` becomes universal for paged views; `cancel_pending_expand` fires for all views with a pending-expand resolver.

A second bug: `force_load_songs_page` in `songs.rs` is a verbatim copy of `load_songs_internal` with the `needs_fetch` gate stripped — already drifted from albums/artists which use a `force: bool` flag in a shared internal. Lane B consolidates this.

Rubric (in priority order):
1. **Bug-class prevention.** The generic body is the single source of truth for the load skeleton. A future "preserve scroll on SSE refresh" change lands in one place; a future view inheriting the trait gets the behavior for free.
2. **Correct bug fixes.** The songs Err path and `selected_indices.retain` fixes are behavioral corrections, not just cosmetic.
3. **Public function names stable.** `handle_albums_loaded`, `handle_albums_page_loaded`, `handle_artists_loaded`, `handle_artists_page_loaded`, `handle_songs_loaded`, `handle_songs_page_loaded`, `handle_genres_loaded`, `handle_playlists_loaded` — all keep their names. They become 1–2 line delegations. Callers in `dispatch_*_loader` fns stay byte-identical.
4. **No new dependencies.** Trait dispatch via zero-sized types; all in std + existing crate imports.

---

## 2. Architecture

### 2.1 `LoaderTarget` trait

**Location**: new `src/update/loader_target.rs`. Declared `pub(crate) mod loader_target;` in `src/update/mod.rs` (alphabetical). Re-exported: `pub(crate) use loader_target::{LoaderTarget, AlbumsTarget, ArtistsTarget, SongsTarget, GenresTarget, PlaylistsTarget};`.

All methods are **associated functions** (take `app: &Nokkvi` or `app: &mut Nokkvi`, not `&self`) so the generic body can call them sequentially without double-borrow on `&mut Nokkvi`.

```rust
/// Per-view hooks that drive the generic `handle_loaded_with` and `handle_page_loaded_with`
/// bodies. All methods are associated functions; implement for a zero-sized marker type.
pub(crate) trait LoaderTarget {
    type Item: Send + 'static;

    // ── Required: library buffer ─────────────────────────────────────────────
    fn library(app: &Nokkvi) -> &PagedBuffer<Self::Item>;
    fn library_mut(app: &mut Nokkvi) -> &mut PagedBuffer<Self::Item>;
    fn count_mut(app: &mut Nokkvi) -> &mut usize;

    // ── Required: page state ────────────────────────────────────────────────
    fn slot_list_mut(app: &mut Nokkvi) -> &mut SlotListView;

    // ── Required: artwork ───────────────────────────────────────────────────
    fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>>;
    fn center_large_artwork_task(app: &mut Nokkvi) -> Option<Task<Message>>;

    // ── Required: pending expand ─────────────────────────────────────────────
    /// `None` for views with no pending-expand chain (Playlists).
    fn try_resolve_pending_expand(app: &mut Nokkvi) -> Option<Task<Message>>;

    // ── Required: identity ───────────────────────────────────────────────────
    fn entity_label() -> &'static str;
    /// Extract the anchor-lookup id. Called by `apply_viewport_on_load` default impl.
    /// Single-shot views override `apply_viewport_on_load` so this is unreachable for them,
    /// but still provide a real impl (e.g., `&item.id`) for safety.
    fn item_id(item: &Self::Item) -> &str;

    // ── Defaulted: viewport (paged view behaviour) ───────────────────────────
    /// Fallback when the anchor id is not found in the newly-loaded buffer.
    /// Default: reset to 0 (Artists / Songs behavior).
    /// AlbumsTarget overrides to clamp to `new_len.saturating_sub(1)`.
    fn anchor_miss_fallback(current_offset: usize, new_len: usize) -> usize {
        let _ = (current_offset, new_len);
        0
    }

    /// Apply viewport state after initial load. Default implements the paged
    /// behavior (Albums / Artists / Songs). GenresTarget and PlaylistsTarget
    /// override to reset to 0 unconditionally without the background branch.
    fn apply_viewport_on_load(app: &mut Nokkvi, background: bool, anchor_id: Option<&str>) {
        let new_len = Self::library(app).len();
        if !background {
            let sl = Self::slot_list_mut(app);
            sl.viewport_offset = 0;
            sl.selected_indices.clear();
        } else {
            let current = Self::slot_list_mut(app).viewport_offset;
            let anchor_idx = anchor_id.and_then(|id| {
                Self::library(app)
                    .iter()
                    .position(|item| Self::item_id(item) == id)
            });
            let new_offset =
                anchor_idx.unwrap_or_else(|| Self::anchor_miss_fallback(current, new_len));
            let sl = Self::slot_list_mut(app);
            sl.viewport_offset = new_offset;
            sl.selected_offset = None;
            sl.selected_indices.retain(|&i| i < new_len);
        }
    }

    // ── Defaulted: error-path cancel ────────────────────────────────────────
    /// Whether to call `cancel_pending_expand()` in the Err branch of the initial
    /// load. Default: true (Albums / Artists / Genres). Songs: currently a bug
    /// (false); the generic body fixes it by defaulting true. PlaylistsTarget
    /// overrides to false (no pending expand to cancel).
    const CANCEL_PENDING_ON_ERR: bool = true;

    // ── Defaulted: post-load hook ─────────────────────────────────────────────
    /// Called after `set_first_page` and viewport reset, before artwork dispatch.
    /// Default: no-op. PlaylistsTarget overrides to refresh the default-playlist picker.
    fn post_load_ok_hook(_app: &mut Nokkvi) {}
}
```

### 2.2 Generic bodies

Both methods are `impl Nokkvi` in `loader_target.rs`:

```rust
impl Nokkvi {
    pub(crate) fn handle_loaded_with<T: LoaderTarget>(
        &mut self,
        result: Result<Vec<T::Item>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        *T::count_mut(self) = total_count;
        match result {
            Ok(items) => {
                debug!(
                    "✅ Loaded {} {}s (total: {})",
                    items.len(), T::entity_label(), total_count
                );
                T::library_mut(self).set_first_page(items, total_count);
                T::apply_viewport_on_load(self, background, anchor_id.as_deref());
                T::post_load_ok_hook(self);

                let mut tasks: Vec<Task<Message>> = T::prefetch_artwork_tasks(self);
                if let Some(task) = T::center_large_artwork_task(self) {
                    tasks.push(task);
                }
                if let Some(task) = T::try_resolve_pending_expand(self) {
                    tasks.push(task);
                }
                if !tasks.is_empty() {
                    return Task::batch(tasks);
                }
            }
            Err(e) => {
                if e.contains("Unauthorized") {
                    T::library_mut(self).set_loading(false);
                    return self.handle_session_expired();
                }
                error!("Error loading {}: {}", T::entity_label(), e);
                T::library_mut(self).set_loading(false);
                if T::CANCEL_PENDING_ON_ERR {
                    self.cancel_pending_expand();
                }
                self.toast_error(format!("Failed to load {}: {e}", T::entity_label()));
            }
        }
        Task::none()
    }

    pub(crate) fn handle_page_loaded_with<T: LoaderTarget>(
        &mut self,
        result: Result<Vec<T::Item>, String>,
        total_count: usize,
    ) -> Task<Message> {
        match result {
            Ok(new_items) => {
                let count = new_items.len();
                let loaded_before = T::library(self).loaded_count();
                T::library_mut(self).append_page(new_items, total_count);
                debug!(
                    "📄 {} page loaded: {} new items ({}→{} of {})",
                    T::entity_label(), count, loaded_before,
                    T::library(self).loaded_count(), total_count,
                );
                if let Some(task) = T::try_resolve_pending_expand(self) {
                    return task;
                }
            }
            Err(e) => {
                if e.contains("Unauthorized") {
                    T::library_mut(self).set_loading(false);
                    return self.handle_session_expired();
                }
                error!("Error loading {} page: {}", T::entity_label(), e);
                T::library_mut(self).set_loading(false);
                self.cancel_pending_expand();
                self.toast_error(format!("Failed to load {}: {e}", T::entity_label()));
            }
        }
        Task::none()
    }
}
```

**Note on `background` / `anchor_id` for single-shot views**: `GenresTarget` and `PlaylistsTarget` override `apply_viewport_on_load` to ignore these params, so the generic body can safely pass `background=false, anchor_id=None` for single-shot dispatch sites (Genres/Playlists `LoaderMessage::Loaded` don't carry these fields). The implementer should pass literal `false, None` at those call sites.

### 2.3 Per-entity specs

Five zero-sized marker structs in `loader_target.rs`:

**`AlbumsTarget`**
- `Item = AlbumUIViewData`; `library = &app.library.albums`; `count_mut = &mut app.library.counts.albums`; `slot_list_mut = &mut app.albums_page.common.slot_list`; `item_id = &item.id`; `label = "Albums"`.
- `anchor_miss_fallback`: `current_offset.min(new_len.saturating_sub(1))` — clamp, not reset.
- `prefetch_artwork_tasks`: call `prefetch_album_artwork_tasks(slot_list, library, cached, shell.albums().clone(), |a| (a.id.clone(), a.artwork_url.clone()))`. Guard `app.app_service.is_some()`.
- `center_large_artwork_task`: `LoadLarge(album.id.clone())` for center index.
- `try_resolve_pending_expand`: `app.try_resolve_pending_expand_album()`.
- `CANCEL_PENDING_ON_ERR = true` (default).

**`ArtistsTarget`**
- `Item = ArtistUIViewData`; `library = &app.library.artists`; `count_mut = &mut app.library.counts.artists`; `slot_list_mut = &mut app.artists_page.common.slot_list`; `item_id = &item.id`; `label = "Artists"`.
- `anchor_miss_fallback`: default (0). Matches current `artists.rs:277` behavior.
- `prefetch_artwork_tasks`: call `app.prefetch_artist_mini_artwork_tasks()` when `app.library.artists.len() > 0 && app.app_service.is_some()`.
- `center_large_artwork_task`: `app.handle_load_artist_large_artwork(artist.id.clone())` for center. This is a `&mut self` call — takes `app: &mut Nokkvi`. Returns `Some(task)`.
- `try_resolve_pending_expand`: `app.try_resolve_pending_expand_artist()`.
- `CANCEL_PENDING_ON_ERR = true` (default).

**`SongsTarget`**
- `Item = SongUIViewData`; `library = &app.library.songs`; `count_mut = &mut app.library.counts.songs`; `slot_list_mut = &mut app.songs_page.common.slot_list`; `item_id = &item.id`; `label = "Songs"`.
- `anchor_miss_fallback`: default (0). Matches current behavior.
- `prefetch_artwork_tasks`: replicate the inline loop from `songs.rs:230-267` — iterate `prefetch_indices(total)`, skip already-cached album IDs, spawn `fetch_album_artwork` tasks via `albums_vm.clone()`. The loop is complex enough to stay inline in the impl rather than call a shared helper. Use `THUMBNAIL_SIZE` (from `nokkvi_data::utils::artwork_url::THUMBNAIL_SIZE`) not the hardcoded `80`.
- `center_large_artwork_task`: `LoadLarge(album_id.clone())` for song at center index. `None` if song has no `album_id`.
- `try_resolve_pending_expand`: `app.try_resolve_pending_expand_song()`.
- `CANCEL_PENDING_ON_ERR = true` — **this is the bug fix**. Current `songs.rs` Err path does NOT call `cancel_pending_expand()`. The generic body corrects it.

**`GenresTarget`**
- `Item = GenreUIViewData`; `library = &app.library.genres`; `count_mut = &mut app.library.counts.genres`; `slot_list_mut = &mut app.genres_page.common.slot_list`; `item_id = &item.id`; `label = "Genres"`.
- `apply_viewport_on_load`: override → `Self::slot_list_mut(app).viewport_offset = 0;` (unconditional reset, no selected_indices handling).
- `prefetch_artwork_tasks`: emit `Task::done(Message::Artwork(ArtworkMessage::StartCollagePrefetch(CollageTarget::Genre)))` + `Task::done(Message::Genres(views::GenresMessage::SlotListSetOffset(0, iced::keyboard::Modifiers::default())))` when library is non-empty.
- `center_large_artwork_task`: `None` — center artwork comes via `SlotListSetOffset(0)`.
- `try_resolve_pending_expand`: `app.try_resolve_pending_expand_genre()`.
- `CANCEL_PENDING_ON_ERR = true` (default — genres have pending expand).

**`PlaylistsTarget`**
- `Item = PlaylistUIViewData`; `library = &app.library.playlists`; `count_mut = &mut app.library.counts.playlists`; `slot_list_mut = &mut app.playlists_page.common.slot_list`; `item_id = &item.id`; `label = "Playlists"`.
- `apply_viewport_on_load`: override → `Self::slot_list_mut(app).viewport_offset = 0;` (same as Genres).
- `post_load_ok_hook`: `app.refresh_default_playlist_picker_after_load()`.
- `prefetch_artwork_tasks`: emit `StartCollagePrefetch(CollageTarget::Playlist)` + `SlotListSetOffset(0)` when library is non-empty.
- `center_large_artwork_task`: `None`.
- `try_resolve_pending_expand`: `None` — playlists have no pending-expand chain.
- `CANCEL_PENDING_ON_ERR = false` — playlists have no pending expand to cancel.

### 2.4 `force_load_songs_page` consolidation (Lane B)

Current: `force_load_songs_page` in `songs.rs:107-153` is a verbatim copy of `load_songs_internal` with the `needs_fetch` gate stripped. Albums and Artists already use a `force: bool` parameter on `load_*_internal`.

Migration:
1. Add `force: bool` to `fn load_songs_internal(&mut self, offset: usize, force: bool, msg_ctor: ...)`.
2. Inside, gate the `needs_fetch` check on `!force` (match albums/artists pattern exactly).
3. `handle_songs_load_page` passes `force: false`. `force_load_songs_page` becomes:
   ```rust
   pub(crate) fn force_load_songs_page(&mut self, offset: usize) -> Task<Message> {
       self.load_songs_internal(offset, true, |(result, total_count)| {
           Message::SongsLoader(SongsLoaderMessage::PageLoaded(result, total_count))
       })
   }
   ```
4. Delete the old `force_load_songs_page` body (removes ~45 LOC of duplication).

---

## 3. Lane decomposition (3 lanes, 2 waves)

| Wave | Lane | Scope | Files owned | Commits (est.) | Effort |
|---|---|---|---|---|---|
| **1** | **A** | Trait + generics + all 5 specs | new `src/update/loader_target.rs`, `src/update/mod.rs` | 4–5 | M |
| **2** | **B** | Paged migrations + songs consolidation | `src/update/albums.rs`, `src/update/artists.rs`, `src/update/songs.rs` | 4–5 | S |
| **2** | **C** | Single-shot migrations + audit tracker | `src/update/genres.rs`, `src/update/playlists.rs`, `.agent/audit-progress.md` | 2–3 | S |

**Wave ordering**: Lane A must merge into `main` before Lanes B and C start. Lanes B and C are file-disjoint and can run simultaneously. Lane A is fully additive (no existing code changed) → merge is always a fast-forward with no conflicts. Lanes B and C are purely subtractive (body replacements) in their respective files → no conflicts between them.

---

## 4. Conflict zones

| Pair | Zone | Resolution |
|---|---|---|
| A ↔ B | `src/update/mod.rs` (mod declaration only) | Lane A adds it; Lane B never touches `mod.rs` — rebase is trivial. |
| A ↔ C | Same as above | Same; Lane C never touches `mod.rs`. |
| B ↔ C | None — fully disjoint files | No rebase needed; merge either order. |

---

## 5. Per-lane scope

### Lane A — foundation

**Files**:
- New `src/update/loader_target.rs` — trait + both generic bodies + all 5 specs.
- `src/update/mod.rs` — add `mod loader_target; pub(crate) use loader_target::{LoaderTarget, AlbumsTarget, ArtistsTarget, SongsTarget, GenresTarget, PlaylistsTarget};`.

**Steps**:
1. Add module declaration to `update/mod.rs`.
2. Create `loader_target.rs` with trait definition + blank impls so the file compiles.
3. Fill in `handle_loaded_with` generic body (no impl calls yet — all trait methods will return stubs).
4. Fill in `handle_page_loaded_with` generic body.
5. Implement all 5 specs one at a time, verifying `cargo build` after each.
6. No existing handler bodies are changed — this is purely additive.

**Commit sequence**:
1. `feat(update): add LoaderTarget trait skeleton and both generic bodies` — trait definition + `handle_loaded_with` + `handle_page_loaded_with`; no spec impls yet.
2. `feat(update): implement AlbumsTarget and ArtistsTarget LoaderTarget specs`.
3. `feat(update): implement SongsTarget LoaderTarget spec`.
4. `feat(update): implement GenresTarget and PlaylistsTarget LoaderTarget specs`.
5. (optional) `refactor(update): consolidate loader_target.rs imports` — clean up after all specs land.

**Merge trigger**: after all 5 commits pass the four-step verify, merge into `main`:
```bash
cd ~/nokkvi
git merge --ff-only refactor/loader-target-foundation
git push
```

### Lane B — paged migrations

**Prerequisite**: Lane A merged into `main`. Lane B starts by `git rebase origin/main` to pick up the `loader_target.rs` module.

**Files**:
- `src/update/albums.rs` — `handle_albums_loaded` and `handle_albums_page_loaded`.
- `src/update/artists.rs` — `handle_artists_loaded` and `handle_artists_page_loaded`.
- `src/update/songs.rs` — `load_songs_internal` (add `force` param), `force_load_songs_page` (collapse), `handle_songs_loaded`, `handle_songs_page_loaded`.

**Delegations** (after migration, each body is ≤ 2 lines):

```rust
// albums.rs
pub(crate) fn handle_albums_loaded(&mut self, result: Result<Vec<AlbumUIViewData>, String>, total_count: usize, background: bool, anchor_id: Option<String>) -> Task<Message> {
    self.handle_loaded_with::<AlbumsTarget>(result, total_count, background, anchor_id)
}
pub(crate) fn handle_albums_page_loaded(&mut self, result: Result<Vec<AlbumUIViewData>, String>, total_count: usize) -> Task<Message> {
    self.handle_page_loaded_with::<AlbumsTarget>(result, total_count)
}
// artists.rs and songs.rs: identical shapes with their respective types
```

**Songs-specific**: consolidate `force_load_songs_page` first (before migrating `handle_songs_loaded`) so the consolidation commit is clean.

**Commit sequence**:
1. `refactor(songs): consolidate force_load_songs_page into load_songs_internal` — add `force: bool` param, delete old body.
2. `refactor(albums): migrate handle_albums_loaded/page_loaded to LoaderTarget`.
3. `refactor(artists): migrate handle_artists_loaded/page_loaded to LoaderTarget`.
4. `refactor(songs): migrate handle_songs_loaded/page_loaded to LoaderTarget` — note: bug fix (cancel_pending_expand on Err now fires) in commit body.

**Merge**: `cd ~/nokkvi && git merge --ff-only refactor/loader-target-paged && git push`.

### Lane C — single-shot migrations

**Prerequisite**: Lane A merged into `main`. Same rebase step as Lane B.

**Files**:
- `src/update/genres.rs` — `handle_genres_loaded`.
- `src/update/playlists.rs` — `handle_playlists_loaded`.
- `.agent/audit-progress.md` — close §7 #8 + §3 #2.

**Delegations**:

```rust
// genres.rs — pass background=false, anchor_id=None (GenresTarget ignores them)
pub(crate) fn handle_genres_loaded(&mut self, result: Result<Vec<GenreUIViewData>, String>, total_count: usize) -> Task<Message> {
    self.handle_loaded_with::<GenresTarget>(result, total_count, false, None)
}

// playlists.rs — same pattern
pub(crate) fn handle_playlists_loaded(&mut self, result: Result<Vec<PlaylistUIViewData>, String>, total_count: usize) -> Task<Message> {
    self.handle_loaded_with::<PlaylistsTarget>(result, total_count, false, None)
}
```

**Commit sequence**:
1. `refactor(genres): migrate handle_genres_loaded to LoaderTarget`.
2. `refactor(playlists): migrate handle_playlists_loaded to LoaderTarget`.
3. `docs(audit): close §7 #8 / DRY #2 after LoaderTarget migration lands`.

**Merge**: `cd ~/nokkvi && git merge --ff-only refactor/loader-target-single-shot && git push`.

---

## 6. Verification (every lane, every commit)

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass before each commit. `cargo test` is the key gate: the existing handler tests exercise the bodies through the dispatch path.

**Lane A specific**: after all 5 specs land, `grep -c 'impl LoaderTarget for' src/update/loader_target.rs` should equal `5`. The `handle_albums_loaded` etc. are NOT touched — their tests still exercise the old bodies and should still pass.

**Lane B specific**: after migration, `wc -l src/update/albums.rs` should be reduced by ~130 LOC (the bodies replaced). Grep: `grep -c 'handle_loaded_with\|handle_page_loaded_with' src/update/albums.rs` should equal `2`.

**Lane C specific**: after migration, `grep -c 'handle_loaded_with' src/update/genres.rs src/update/playlists.rs` should equal `2` (one per file).

---

## 7. What each lane does NOT do

- No public function renames. `handle_albums_loaded`, `dispatch_albums_loader`, `handle_session_expired`, `cancel_pending_expand` keep their names and signatures.
- No new behavior. The generic body is behavior-equivalent to the pre-migration bodies for all 5 views, modulo the two bug fixes (songs Err path + `selected_indices.retain`).
- No changes to `dispatch_*_loader` call sites — they call `handle_albums_loaded(...)` etc. which now delegates.
- No migration of Queue / Radios / Similar loaded handlers — those are structurally different (Queue is in-place, Radios has no `*LoaderMessage`, Similar has a generation token). Out of scope.
- No test changes — existing dispatch tests exercise via the same call chain.
- No `.agent/rules/` edits.

---

## Fanout Prompts

### lane-a-foundation

worktree: ~/nokkvi-loader-a
branch: refactor/loader-target-foundation
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane A of the LoaderTarget refactor — create `src/update/loader_target.rs` with the `LoaderTarget` trait, both generic handler bodies, and all 5 per-entity spec implementations. Do NOT change any existing handler bodies — this is purely additive.

Plan doc: /home/foogs/nokkvi/.agent/plans/loader-target-trait.md (sections 2 and 5 "Lane A").

Working directory: ~/nokkvi-loader-a (this worktree). Branch: refactor/loader-target-foundation. The worktree is already created — do NOT run `git worktree add`.

## Baseline verification

```bash
git log -1 --oneline   # should show current main HEAD
ls src/update/loader_target.rs 2>/dev/null && echo EXISTS || echo ABSENT   # should print ABSENT
grep -n 'mod loader_target' src/update/mod.rs    # should return nothing
```

## Step 1: Wire the module

In `src/update/mod.rs`, add (alphabetical order among existing `mod` declarations):
```rust
mod loader_target;
pub(crate) use loader_target::{AlbumsTarget, ArtistsTarget, GenresTarget, LoaderTarget, PlaylistsTarget, SongsTarget};
```

## Step 2: Create loader_target.rs

Create `src/update/loader_target.rs`. Start with imports — you will need (read the existing update files to confirm exact import paths):
- `crate::{Message, Nokkvi}` 
- `crate::app_message::ArtworkMessage` (for `LoadLarge`, `StartCollagePrefetch`)
- `crate::state::CollageTarget` (or wherever it's imported from in genres.rs / playlists.rs — grep it)
- `crate::views` (for `GenresMessage`, `PlaylistsMessage`, `SlotListSetOffset`)
- `crate::update::components::prefetch_album_artwork_tasks`
- `iced::Task`
- `iced::widget::image`
- `nokkvi_data::types::paged_buffer::PagedBuffer`
- `nokkvi_data::utils::artwork_url::THUMBNAIL_SIZE`
- `crate::widgets::slot_list_view::SlotListView` (or wherever SlotListView is re-exported)
- `std::collections::HashSet`
- `tracing::{debug, error}`

Run `cargo build` after each major section to catch import errors early.

## Step 3: Define the trait

Copy the trait definition exactly from the plan (§2.1). Key points:
- All methods are associated functions, not instance methods.
- `CANCEL_PENDING_ON_ERR: bool = true` is a provided associated constant.
- `anchor_miss_fallback(current_offset: usize, new_len: usize) -> usize` takes NO `app` param.
- `apply_viewport_on_load` default impl handles paged views; single-shot views override it.
- `post_load_ok_hook` default is `fn post_load_ok_hook(_app: &mut Nokkvi) {}`.

## Step 4: Implement the two generic bodies

Copy `handle_loaded_with` and `handle_page_loaded_with` from §2.2. Both are `impl Nokkvi` in this file. Run `cargo build` — it will compile even without any `impl LoaderTarget for ...` blocks because the generics are not called yet.

## Step 5: Implement all 5 specs

Work in this order: Albums → Artists → Songs → Genres → Playlists. After each impl, run `cargo build` to catch errors before moving on.

### AlbumsTarget

```rust
pub(crate) struct AlbumsTarget;
impl LoaderTarget for AlbumsTarget {
    type Item = crate::views::albums::AlbumUIViewData;
    fn library(app: &Nokkvi) -> &PagedBuffer<Self::Item> { &app.library.albums }
    fn library_mut(app: &mut Nokkvi) -> &mut PagedBuffer<Self::Item> { &mut app.library.albums }
    fn count_mut(app: &mut Nokkvi) -> &mut usize { &mut app.library.counts.albums }
    fn slot_list_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.albums_page.common.slot_list
    }
    fn item_id(item: &Self::Item) -> &str { &item.id }
    fn entity_label() -> &'static str { "Albums" }

    fn anchor_miss_fallback(current_offset: usize, new_len: usize) -> usize {
        current_offset.min(new_len.saturating_sub(1))
    }

    fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>> {
        let Some(shell) = &app.app_service else { return vec![]; };
        let cached: HashSet<&String> = app.artwork.album_art.iter().map(|(k, _)| k).collect();
        prefetch_album_artwork_tasks(
            &app.albums_page.common.slot_list,
            &app.library.albums,
            &cached,
            shell.albums().clone(),
            |album| (album.id.clone(), album.artwork_url.clone()),
        )
    }

    fn center_large_artwork_task(app: &mut Nokkvi) -> Option<Task<Message>> {
        let total = app.library.albums.len();
        let center_idx = app.albums_page.common.slot_list.get_center_item_index(total)?;
        let album_id = app.library.albums.get(center_idx)?.id.clone();
        Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(album_id))))
    }

    fn try_resolve_pending_expand(app: &mut Nokkvi) -> Option<Task<Message>> {
        app.try_resolve_pending_expand_album()
    }
}
```

### ArtistsTarget

Same shape. Key differences:
- `library = &app.library.artists`, `count_mut = &mut app.library.counts.artists`, `slot_list_mut = &mut app.artists_page.common.slot_list`, `item_id = &item.id`, `label = "Artists"`.
- `anchor_miss_fallback`: default (0).
- `prefetch_artwork_tasks`: 
  ```rust
  fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>> {
      if app.library.artists.is_empty() || app.app_service.is_none() { return vec![]; }
      vec![app.prefetch_artist_mini_artwork_tasks()]
  }
  ```
- `center_large_artwork_task`:
  ```rust
  fn center_large_artwork_task(app: &mut Nokkvi) -> Option<Task<Message>> {
      let total = app.library.artists.len();
      if total == 0 || app.app_service.is_none() { return None; }
      let center_idx = app.artists_page.common.slot_list.get_center_item_index(total)?;
      let artist_id = app.library.artists.get(center_idx)?.id.clone();
      Some(app.handle_load_artist_large_artwork(artist_id))
  }
  ```
- `try_resolve_pending_expand`: `app.try_resolve_pending_expand_artist()`.

### SongsTarget

Same shape. Key differences:
- `library = &app.library.songs`, `count_mut = &mut app.library.counts.songs`, `slot_list_mut = &mut app.songs_page.common.slot_list`, `item_id = &item.id`, `label = "Songs"`.
- `anchor_miss_fallback`: default (0).
- `prefetch_artwork_tasks`: replicate the loop from `src/update/songs.rs:230-267`. Use `THUMBNAIL_SIZE` not `80`. Emit `Message::Artwork(ArtworkMessage::SongMiniLoaded(id, handle))`.
- `center_large_artwork_task`:
  ```rust
  fn center_large_artwork_task(app: &mut Nokkvi) -> Option<Task<Message>> {
      let total = app.library.songs.len();
      let center_idx = app.songs_page.common.slot_list.get_center_item_index(total)?;
      let album_id = app.library.songs.get(center_idx)?.album_id.as_ref()?.clone();
      Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(album_id))))
  }
  ```
- `try_resolve_pending_expand`: `app.try_resolve_pending_expand_song()`.
- `CANCEL_PENDING_ON_ERR = true` (default — this is the bug fix; existing body has false).

### GenresTarget

```rust
pub(crate) struct GenresTarget;
impl LoaderTarget for GenresTarget {
    type Item = /* GenreUIViewData — check import path from genres.rs */;
    fn library(app: &Nokkvi) -> &PagedBuffer<Self::Item> { &app.library.genres }
    fn library_mut(app: &mut Nokkvi) -> &mut PagedBuffer<Self::Item> { &mut app.library.genres }
    fn count_mut(app: &mut Nokkvi) -> &mut usize { &mut app.library.counts.genres }
    fn slot_list_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.genres_page.common.slot_list
    }
    fn item_id(item: &Self::Item) -> &str { &item.id }
    fn entity_label() -> &'static str { "Genres" }

    // Override: single-shot — always reset, no anchor handling
    fn apply_viewport_on_load(app: &mut Nokkvi, _background: bool, _anchor_id: Option<&str>) {
        Self::slot_list_mut(app).viewport_offset = 0;
    }

    fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>> {
        let mut tasks = Vec::new();
        tasks.push(Task::done(Message::Artwork(ArtworkMessage::StartCollagePrefetch(
            CollageTarget::Genre,
        ))));
        if !app.library.genres.is_empty() {
            tasks.push(Task::done(Message::Genres(
                crate::views::GenresMessage::SlotListSetOffset(
                    0,
                    iced::keyboard::Modifiers::default(),
                ),
            )));
        }
        tasks
    }

    fn center_large_artwork_task(_app: &mut Nokkvi) -> Option<Task<Message>> { None }

    fn try_resolve_pending_expand(app: &mut Nokkvi) -> Option<Task<Message>> {
        app.try_resolve_pending_expand_genre()
    }
    // CANCEL_PENDING_ON_ERR = true (default, genres have pending expand)
}
```

### PlaylistsTarget

Same as GenresTarget but for playlists. Key differences:
- `library.playlists`, `counts.playlists`, `playlists_page`, `PlaylistsMessage::SlotListSetOffset`, `CollageTarget::Playlist`.
- `post_load_ok_hook`:
  ```rust
  fn post_load_ok_hook(app: &mut Nokkvi) {
      app.refresh_default_playlist_picker_after_load();
  }
  ```
- `try_resolve_pending_expand`: `None` — playlists have no pending expand.
  ```rust
  fn try_resolve_pending_expand(_app: &mut Nokkvi) -> Option<Task<Message>> { None }
  ```
- `CANCEL_PENDING_ON_ERR = false`:
  ```rust
  const CANCEL_PENDING_ON_ERR: bool = false;
  ```

## Borrow-checker notes

If the compiler complains about simultaneous borrows in the default `apply_viewport_on_load`:
- `Self::library(app).len()` creates a temporary `&PagedBuffer` that is immediately dropped (`.len()` copies a `usize`). By the time `Self::slot_list_mut(app)` is called, the borrow is gone. This is standard Rust — the calls are sequential, not simultaneous.
- If you still get an error, extract the len read first: `let new_len = Self::library(app).len();` on its own statement before `Self::slot_list_mut(app)`.

If `center_large_artwork_task` for `ArtistsTarget` has a borrow issue (it calls `app.handle_load_artist_large_artwork(id)` which is `&mut self`): that method mutates `app.artwork.loading_large_artwork`. Since we're passing `app: &mut Nokkvi` into the trait fn, this is fine — the entire `app` is mutably borrowed for the duration of `center_large_artwork_task`. There is no simultaneous borrow conflict.

## Verification (after each spec and final)

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass. Since no existing handler bodies are changed, the existing tests all pass unchanged.

## Commit slices

1. `feat(update): add LoaderTarget trait skeleton and both generic bodies` — `loader_target.rs` + `mod.rs` wiring; no spec impls.
2. `feat(update): implement AlbumsTarget and ArtistsTarget LoaderTarget specs`.
3. `feat(update): implement SongsTarget LoaderTarget spec`.
4. `feat(update): implement GenresTarget and PlaylistsTarget LoaderTarget specs`.

## Merge into main

After all four commits pass verification:

```bash
cd ~/nokkvi
git merge --ff-only refactor/loader-target-foundation
git push
```

If `--ff-only` fails (main has moved ahead — unlikely since Lane A is the first wave), rebase first:
```bash
cd ~/nokkvi-loader-a && git rebase origin/main
cd ~/nokkvi && git merge --ff-only refactor/loader-target-foundation && git push
```

## What NOT to touch

- Any existing handler body in `albums.rs`, `artists.rs`, `songs.rs`, `genres.rs`, `playlists.rs` — Lane B and C's territory.
- Any `dispatch_*_loader` function — callers stay unchanged.
- `.agent/audit-progress.md` — Lane C closes the item.
- `.agent/rules/` files.

## If blocked

- If a type path can't be resolved (e.g., `GenreUIViewData` import path): grep the existing `update/genres.rs` imports at the top of the file and mirror them.
- If `prefetch_album_artwork_tasks` signature differs from what the plan says: read `src/update/components.rs` for its actual signature and adapt.
- If `CollageTarget` isn't importable from `loader_target.rs`: check where it's imported in `genres.rs` and use the same path.
- If any trait method's default impl causes a borrow error that can't be resolved sequentially: move the problematic logic into a free fn that takes explicit field references by name (bypassing the trait method for internal use), and have the default impl call that free fn.

## Reporting

End with: commits (refs + subjects), `grep -c 'impl LoaderTarget for' src/update/loader_target.rs` (should be 5), `cargo test` pass/fail, any trait method that needed a non-plan shape (one sentence each).
````

---

### lane-b-paged-migrations

worktree: ~/nokkvi-loader-b
branch: refactor/loader-target-paged
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane B of the LoaderTarget refactor — migrate `handle_albums_loaded`, `handle_albums_page_loaded`, `handle_artists_loaded`, `handle_artists_page_loaded`, `handle_songs_loaded`, `handle_songs_page_loaded` to delegate to the generic bodies; also consolidate `force_load_songs_page` into `load_songs_internal`.

Plan doc: /home/foogs/nokkvi/.agent/plans/loader-target-trait.md (sections 2.4 and 5 "Lane B").

Working directory: ~/nokkvi-loader-b (this worktree). Branch: refactor/loader-target-paged. The worktree is already created — do NOT run `git worktree add`.

**PREREQUISITE**: Lane A (branch `refactor/loader-target-foundation`) must be merged into `main` before you start the migrations. Check:

```bash
git fetch origin
git log --oneline origin/main | head -8
grep -n 'mod loader_target' ~/nokkvi/src/update/mod.rs
```

If `mod loader_target` is not in `mod.rs`, Lane A has not merged yet. Rebase and wait:
```bash
git rebase origin/main
```
Check again. Proceed only when `mod loader_target` appears in `mod.rs`.

## After confirming Lane A has merged

```bash
git rebase origin/main   # pick up loader_target.rs
cargo build              # must pass before any migration
```

## Step 1: Consolidate force_load_songs_page

In `src/update/songs.rs`, locate `fn load_songs_internal` (the private fn used by `handle_load_songs` and `handle_songs_load_page`). Add a `force: bool` parameter:

```rust
fn load_songs_internal<F>(&mut self, offset: usize, force: bool, msg_ctor: F) -> Task<Message>
where F: Fn((Result<Vec<SongUIViewData>, String>, usize)) -> Message + Send + 'static
```

Inside, find the `needs_fetch` gate (it currently rejects loads when `needs_fetch()` returns `None` — check the exact guard by reading the existing body). Wrap it: `if !force { /* existing gate */ }`. Match the exact pattern used in `src/update/albums.rs::load_albums_internal` and `src/update/artists.rs::load_artists_internal` (read both to confirm).

Update the two existing callers:
- `handle_load_songs`: passes `force: false`
- `handle_songs_load_page`: passes `force: false`

Replace `force_load_songs_page` body (currently ~45 LOC) with:
```rust
pub(crate) fn force_load_songs_page(&mut self, offset: usize) -> Task<Message> {
    self.load_songs_internal(offset, true, |(result, total_count)| {
        Message::SongsLoader(crate::app_message::SongsLoaderMessage::PageLoaded(
            result, total_count,
        ))
    })
}
```

Run `cargo build && cargo test`. Commit: `refactor(songs): consolidate force_load_songs_page into load_songs_internal`.

## Step 2: Migrate albums

In `src/update/albums.rs`:

Replace `handle_albums_loaded` body with:
```rust
pub(crate) fn handle_albums_loaded(
    &mut self,
    result: Result<Vec<AlbumUIViewData>, String>,
    total_count: usize,
    background: bool,
    anchor_id: Option<String>,
) -> Task<Message> {
    self.handle_loaded_with::<AlbumsTarget>(result, total_count, background, anchor_id)
}
```

Replace `handle_albums_page_loaded` body with:
```rust
pub(crate) fn handle_albums_page_loaded(
    &mut self,
    result: Result<Vec<AlbumUIViewData>, String>,
    total_count: usize,
) -> Task<Message> {
    self.handle_page_loaded_with::<AlbumsTarget>(result, total_count)
}
```

Delete the old multi-line bodies. Verify: `wc -l src/update/albums.rs` should drop by ~130 lines. Remove any imports that are now dead (e.g., `HashSet` if only used by the old body — check with `cargo clippy`).

Run `cargo build && cargo test && cargo clippy --all-targets -- -D warnings`. Commit: `refactor(albums): migrate handle_albums_loaded/page_loaded to LoaderTarget`.

## Step 3: Migrate artists

Identical pattern:
```rust
pub(crate) fn handle_artists_loaded(&mut self, result: Result<Vec<ArtistUIViewData>, String>, total_count: usize, background: bool, anchor_id: Option<String>) -> Task<Message> {
    self.handle_loaded_with::<ArtistsTarget>(result, total_count, background, anchor_id)
}
pub(crate) fn handle_artists_page_loaded(&mut self, result: Result<Vec<ArtistUIViewData>, String>, total_count: usize) -> Task<Message> {
    self.handle_page_loaded_with::<ArtistsTarget>(result, total_count)
}
```

Run `cargo build && cargo test && cargo clippy --all-targets -- -D warnings`. Commit: `refactor(artists): migrate handle_artists_loaded/page_loaded to LoaderTarget`.

## Step 4: Migrate songs

Identical pattern:
```rust
pub(crate) fn handle_songs_loaded(&mut self, result: Result<Vec<SongUIViewData>, String>, total_count: usize, background: bool, anchor_id: Option<String>) -> Task<Message> {
    self.handle_loaded_with::<SongsTarget>(result, total_count, background, anchor_id)
}
pub(crate) fn handle_songs_page_loaded(&mut self, result: Result<Vec<SongUIViewData>, String>, total_count: usize) -> Task<Message> {
    self.handle_page_loaded_with::<SongsTarget>(result, total_count)
}
```

Note in the commit body: "fix: songs Err path now calls cancel_pending_expand (was missing — baked in by LoaderTarget default CANCEL_PENDING_ON_ERR = true)".

Run `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo +nightly fmt --all -- --check`. Commit: `refactor(songs): migrate handle_songs_loaded/page_loaded to LoaderTarget`.

## After all 4 commits

Verify:
```bash
grep -c 'handle_loaded_with\|handle_page_loaded_with' src/update/albums.rs   # should be 2
grep -c 'handle_loaded_with\|handle_page_loaded_with' src/update/artists.rs  # should be 2
grep -c 'handle_loaded_with\|handle_page_loaded_with' src/update/songs.rs    # should be 2
grep -c 'fn expand_.*_timeout_task\|fn force_load_songs_page' src/update/songs.rs  # force_load_songs_page KEPT (name preserved), body should be 3 lines
```

## Merge into main

```bash
cd ~/nokkvi
git fetch
git merge --ff-only refactor/loader-target-paged
git push
```

If `--ff-only` fails because Lane C also merged:
```bash
cd ~/nokkvi-loader-b && git rebase origin/main
cd ~/nokkvi && git merge --ff-only refactor/loader-target-paged && git push
```

## What NOT to touch

- `src/update/genres.rs` and `src/update/playlists.rs` — Lane C's territory.
- `src/update/loader_target.rs` — Lane A's territory.
- `dispatch_*_loader` functions — callers stay unchanged.
- `.agent/audit-progress.md` — Lane C closes the item.

## If blocked

- If `load_songs_internal` doesn't have a `needs_fetch` gate (i.e., songs already loads unconditionally): skip the `force` param; just inline `force_load_songs_page` body into the function and delete the duplication.
- If `cargo test` reveals a behavioral regression in the songs Err path (specifically a test that EXPECTED `cancel_pending_expand` NOT to fire): report it; don't suppress. The audit identified missing `cancel_pending_expand` as a bug; if a test expects the bug, the test is wrong.
- If any migration causes an import error (e.g., `AlbumsTarget` not in scope): add `use crate::update::{AlbumsTarget, ArtistsTarget, SongsTarget};` at the top of the migrated file.

## Reporting

End with: 4 commit refs + subjects, `wc -l` delta for each of the 3 files, test count delta (should be 0), any behavioral difference from the original body (one sentence each).
````

---

### lane-c-single-shot-migrations

worktree: ~/nokkvi-loader-c
branch: refactor/loader-target-single-shot
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane C of the LoaderTarget refactor — migrate `handle_genres_loaded` and `handle_playlists_loaded` to the generic body, then update the audit tracker.

Plan doc: /home/foogs/nokkvi/.agent/plans/loader-target-trait.md (sections 2 and 5 "Lane C").

Working directory: ~/nokkvi-loader-c (this worktree). Branch: refactor/loader-target-single-shot. The worktree is already created — do NOT run `git worktree add`.

**PREREQUISITE**: Lane A (branch `refactor/loader-target-foundation`) must be merged into `main` before the migrations. Check:

```bash
git fetch origin
grep -n 'mod loader_target' ~/nokkvi/src/update/mod.rs
```

If absent, rebase: `git rebase origin/main`. Proceed only when present.

```bash
git rebase origin/main
cargo build   # must pass before migrating
```

## Step 1: Migrate genres

In `src/update/genres.rs`, replace `handle_genres_loaded` body with:

```rust
pub(crate) fn handle_genres_loaded(
    &mut self,
    result: Result<Vec<GenreUIViewData>, String>,
    total_count: usize,
) -> Task<Message> {
    self.handle_loaded_with::<GenresTarget>(result, total_count, false, None)
}
```

`background=false` and `anchor_id=None` are intentional — `GenresTarget::apply_viewport_on_load` overrides the default and ignores both params; passing them is required by the signature.

Delete the old multi-line body. Add `use crate::update::GenresTarget;` if needed. Remove any now-dead imports.

Run `cargo build && cargo test && cargo clippy --all-targets -- -D warnings`. Commit: `refactor(genres): migrate handle_genres_loaded to LoaderTarget`.

## Step 2: Migrate playlists

Same pattern in `src/update/playlists.rs`:

```rust
pub(crate) fn handle_playlists_loaded(
    &mut self,
    result: Result<Vec<PlaylistUIViewData>, String>,
    total_count: usize,
) -> Task<Message> {
    self.handle_loaded_with::<PlaylistsTarget>(result, total_count, false, None)
}
```

`PlaylistsTarget::post_load_ok_hook` calls `refresh_default_playlist_picker_after_load()` automatically — you do NOT need to call it manually here.

Run `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo +nightly fmt --all -- --check`. Commit: `refactor(playlists): migrate handle_playlists_loaded to LoaderTarget`.

## Step 3: Update audit tracker

In `.agent/audit-progress.md`:

1. Find §7 row 8 (`Loader-result LoaderTarget trait`). Change `❌ open` to `✅ done`. Append commit refs and a brief note: "Three-lane fanout (2026-05-09) from `.agent/plans/loader-target-trait.md`: Lane A (foundation) `<lane-a-refs>`, Lane B (paged: `<lane-b-first-ref>`..`<lane-b-last-ref>`), Lane C (single-shot: this commit). Bug fixes baked in: songs Err path now calls `cancel_pending_expand` (was missing); `selected_indices.retain` now applied universally for paged views on background reload; `force_load_songs_page` consolidated into `load_songs_internal`."

   For the Lane A and B refs: grep the git log: `git log --oneline origin/main | head -15`. The loader-target commits should be visible.

2. Find §3 row 2 (`handle_*_loaded` LoaderTarget trait). Change `❌ open` to `✅ done` with same evidence.

3. Update "Quick-pick" section: remove the LoaderTarget item from the open list.

4. Update `Last verified:` date to `2026-05-09`.

Commit: `docs(audit): close §7 #8 / DRY #2 after LoaderTarget migration lands`.

## Merge into main

```bash
cd ~/nokkvi
git fetch
git merge --ff-only refactor/loader-target-single-shot
git push
```

If `--ff-only` fails because Lane B also merged in the interim, rebase:
```bash
cd ~/nokkvi-loader-c && git rebase origin/main
cd ~/nokkvi && git merge --ff-only refactor/loader-target-single-shot && git push
```

## What NOT to touch

- `src/update/albums.rs`, `src/update/artists.rs`, `src/update/songs.rs` — Lane B's territory.
- `src/update/loader_target.rs` — Lane A's territory.
- Any `dispatch_*_loader` function.

## If blocked

- If `GenresTarget` isn't in scope: add `use crate::update::GenresTarget;` (and `PlaylistsTarget`).
- If the playlists migration leaves a dead import for `CollageTarget` or similar: let `cargo clippy` guide removal.
- For the audit tracker: if Lane A or B refs aren't visible in `origin/main` yet (they haven't merged), use placeholder `<lane-a-tbd>` and note that it will be updated when all lanes merge.

## Reporting

End with: 3 commit refs + subjects, `wc -l` delta for genres.rs and playlists.rs, test count delta (should be 0).
````
