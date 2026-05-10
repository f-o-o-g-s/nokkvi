# Pending-expand dedup — fanout plan (§7 #6 / DRY #1 / Drift cross-cutting #1)

Closes `.agent/audit-progress.md` §7 #6. Picks up the highest-leverage *agent-friendliness* refactor in the project: today four near-identical entity copies of the find-and-expand chain (Album / Artist / Genre / Song) live in `src/update/navigation.rs`, with a paired tri-mirror of ~50 tests in `src/update/tests/navigation.rs`. This plan collapses both sides to descriptor-driven shapes so adding a new expandable entity is a one-row table edit, and editing the chain is a one-place change.

Last verified baseline: **2026-05-08, `main @ HEAD = c45258b`** (`update/navigation.rs` 1139 LOC; `tests/navigation.rs` 2037 LOC; 96 tests in nav tests, of which ~50 mirror across album/artist/genre/song).

Source reports: `~/nokkvi-audit-results/{_SYNTHESIS,monoliths-ui,dry-handlers,dry-tests,drift-match-arms}.md`.

---

## 1. Goal & rubric

The pending-expand chain is the **single largest agent-drift surface** in the project. Today the same find-and-expand state machine appears four times in source and three times in tests, with subtle per-entity differences that are easy to mistake for accidental copy errors:

| Variation axis | Album | Artist | Genre | Song |
|---|---|---|---|---|
| Match predicate | `entity.id == target` | `entity.id == target` | `entity.name == target` | `entity.id == target` |
| Pagination | force-load next page | force-load next page | single-shot (idle+missing → fail) | force-load next page |
| FocusAndExpand dispatch | yes | yes | yes | **no** (center-only always) |
| `PendingTopPin` variant | `Album(id)` | `Artist(id)` | `Genre(resolved_id)` | none |
| `*_page.expansion.clear()` on prime | yes | yes | yes | **no** (songs aren't expandable) |
| `center_only` branch | yes | yes | yes | implicit (always) |

A future agent editing one of the four `try_resolve_pending_expand_*` functions has to keep the other three in sync by hand; a future agent adding a new expandable entity (e.g., Playlists) has to remember every fan-out site. Today the audit estimates **~600 LOC of source copy-paste + ~1100 LOC of test mirror**.

Rubric (in order):
1. **Bug-class prevention.** Adding a new expandable entity is one descriptor entry; editing the chain is one place; per-entity quirks (genre name match, song center-only, single-shot vs paginated) become typed data, not body-divergence.
2. **Test signal preservation.** Every `#[test]` in the current `tests/navigation.rs` produces a real `#[test]` after migration — coverage unchanged. Failure isolation per scenario is preserved (no row-loops collapsing scenarios into one test body).
3. **Public function names stable.** `try_resolve_pending_expand_album/artist/genre/song`, `handle_navigate_and_expand_*`, `handle_pending_expand_*_timeout`, `prime_expand_*_target` keep their names. They become thin wrappers / 5-LOC factory calls. Callers in `update/{albums,artists,songs,genres}.rs:131-285` and `main.rs:273-284` stay intact.
4. **Genre's name-vs-id quirk stays visible.** It's a real Navidrome API quirk (`/api/genre` returns internal UUIDs that differ from the click site's `extra_value`), and the resolved internal id IS what gets pinned. The descriptor must encode this explicitly, not bury it.

---

## 2. Architecture

Three concerns, three structures. All three are idiomatic Rust (zero-sized types implementing a trait, or a single-spec match function), no new dependencies.

### 2.1 `ResolveSpec` trait — closes the resolver mirror

**Location**: new module `src/update/pending_expand_resolve.rs` (or top of `update/navigation.rs` resolver section if implementer prefers — see Lane A).

```rust
use std::borrow::Cow;
use crate::{Message, Nokkvi, View, state::{PendingExpand, PendingTopPin}, views, widgets::SlotListPageState};
use nokkvi_data::types::paged_buffer::PagedBuffer;

/// Per-entity hooks driving the generic `try_resolve_pending_expand_with` body.
///
/// The four implementations (`AlbumSpec`, `ArtistSpec`, `GenreSpec`, `SongSpec`)
/// are zero-sized types — the trait is a vtable for the entity's quirks, not
/// runtime polymorphism.
pub(crate) trait ResolveSpec {
    /// Library entity (e.g. AlbumUIViewData).
    type Item;

    /// Pull the target id from a `PendingExpand` if it's the matching variant.
    fn target_id(pending: &PendingExpand) -> Option<String>;

    /// The library buffer this resolver scans.
    fn library(library: &crate::state::LibraryData) -> &PagedBuffer<Self::Item>;

    /// Lookup the slot-list page state.
    fn page_mut(app: &mut Nokkvi) -> &mut SlotListPageState;

    /// Predicate matching one library item against the resolver's target id.
    /// Genre overrides this to match by `name`; others match by `id`.
    /// Returns the *resolved* id to pin (genres' resolved id is the
    /// internal UUID, distinct from the matched name).
    fn match_target(item: &Self::Item, target: &str) -> Option<Cow<'_, str>>;

    /// Build the per-entity FocusAndExpand message. Returns None for Song
    /// (songs are not expandable; resolver only centers).
    fn focus_and_expand(idx: usize) -> Option<Message>;

    /// Wrap the resolved id in a `PendingTopPin` variant. Returns None for Song.
    fn pin(resolved_id: String) -> Option<PendingTopPin>;

    /// Force-load the next page when target isn't in the buffer yet.
    /// Returns None for Genre (single-shot — no more pages will arrive).
    fn force_load_next_page(app: &mut Nokkvi, offset: usize) -> Option<Task<Message>>;

    /// Toast text and warn-log label, e.g. "Album not found in library".
    fn label() -> &'static str;
}

pub(crate) struct AlbumSpec;
pub(crate) struct ArtistSpec;
pub(crate) struct GenreSpec;
pub(crate) struct SongSpec;

impl Nokkvi {
    pub(crate) fn try_resolve_pending_expand_with<S: ResolveSpec>(
        &mut self,
    ) -> Option<Task<Message>> {
        let target_id = S::target_id(self.pending_expand.as_ref()?)?;

        let lib = S::library(&self.library);
        let found = lib.iter().enumerate().find_map(|(i, item)| {
            S::match_target(item, &target_id).map(|resolved| (i, resolved.into_owned()))
        });

        if let Some((idx, resolved_id)) = found {
            let center_only = self.pending_expand_center_only;
            debug!(/* … "[EXPAND] Found {label} '{target}' at index {idx}" … */);
            self.pending_expand = None;
            self.pending_expand_center_only = false;
            let total = S::library(&self.library).len();
            let page = S::page_mut(self);
            let target_offset = if center_only {
                idx
            } else {
                let center_slot = page.slot_count.max(2) / 2;
                idx.saturating_add(center_slot).min(total.saturating_sub(1))
            };
            page.set_offset(target_offset, total);
            page.pin_selected(idx, total);
            page.flash_center();
            let prefetch_task = self.prefetch_viewport_artwork();
            if center_only {
                return Some(prefetch_task);
            }
            self.pending_top_pin = S::pin(resolved_id);
            return Some(match S::focus_and_expand(idx) {
                Some(focus_msg) => Task::batch([prefetch_task, Task::done(focus_msg)]),
                None => prefetch_task,
            });
        }

        if S::library(&self.library).fully_loaded() {
            warn!(/* "[EXPAND] {label} '{target}' not found after full load" */);
            self.toast_warn(format!("{} not found in library", S::label()));
            self.pending_expand = None;
            self.pending_expand_center_only = false;
            return Some(Task::none());
        }
        if S::library(&self.library).is_loading() {
            return None;
        }

        let next_offset = S::library(&self.library).loaded_count();
        S::force_load_next_page(self, next_offset)
            .or_else(|| {
                // Single-shot (Genre): idle + not-found is terminal.
                warn!(/* "[EXPAND] {label} '{target}' not found after load" */);
                self.toast_warn(format!("{} not found in library", S::label()));
                self.pending_expand = None;
                self.pending_expand_center_only = false;
                Some(Task::none())
            })
    }
}

impl Nokkvi {
    pub(crate) fn try_resolve_pending_expand_album(&mut self) -> Option<Task<Message>> {
        self.try_resolve_pending_expand_with::<AlbumSpec>()
    }
    // … artist / genre / song wrappers identical shape …
}
```

**Genre quirk encoding**: `GenreSpec::match_target` returns `Some(Cow::Owned(item.id.clone()))` when `item.name == target` — the *resolved* internal id is what gets pinned, not the matched name. Album/Artist/Song use `Some(Cow::Borrowed(target))` since they match by id. The trait method returning `Option<Cow<'_, str>>` keeps the no-allocation path for the id-match cases.

**Why one big method, not 4 thin wrappers around per-section helpers**: the body has 5 disjoint exits (found+focus, found+center, fully-loaded miss, loading, idle+force-load, idle+single-shot-miss). Splitting them across helpers would re-introduce the parallel-mirror class on a smaller scale. One spec-driven body keeps the state-machine visible.

**Genre's "single-shot idle terminal" pre-audit comment** (`navigation.rs:706`):
> "Single-shot: idle + not-found means the genre genuinely isn't in the library — no more pages will arrive."

This becomes encoded as `GenreSpec::force_load_next_page() -> None`. The fallback `or_else` arm in the generic body emits the same warn + toast.

### 2.2 Priming + timeout helpers — closes the handler mirror

**Location**: same module as 2.1, or a sibling — see Lane B.

The four `prime_expand_*_target` bodies are 7-9 line state resets that differ only by which `*_page` field they reach into. Same for the four `handle_pending_expand_*_timeout` and the four `expand_*_timeout_task` free functions. These are too small to justify a trait — a single function dispatching on `PendingExpand` variants is enough:

```rust
/// Single source of truth for the priming state-reset.
///
/// Called by every `handle_navigate_and_expand_*` / `handle_browser_pane_navigate_and_expand_*` /
/// `start_center_on_playing_*_chain` site after they decide which entity is being targeted.
///
/// Differences across entities are the page-state field the reset hits. Songs additionally
/// skip `expansion.clear()` because songs aren't expandable.
pub(crate) fn prime_expand_target(app: &mut Nokkvi, pending: PendingExpand) {
    match &pending {
        PendingExpand::Album { .. } => {
            let p = &mut app.albums_page.common;
            p.search_input_focused = false;
            p.active_filter = None;
            p.search_query.clear();
            app.albums_page.expansion.clear();
            p.slot_list.viewport_offset = 0;
            p.slot_list.selected_indices.clear();
            p.slot_list.selected_offset = None;
            app.library.albums.clear();
        }
        PendingExpand::Artist { .. } => { /* artists_page + library.artists; with expansion.clear() */ }
        PendingExpand::Genre { .. } => { /* genres_page + library.genres; with expansion.clear() */ }
        PendingExpand::Song { .. } => {
            let p = &mut app.songs_page.common;
            p.search_input_focused = false;
            p.active_filter = None;
            p.search_query.clear();
            // NB: songs aren't expandable — no expansion.clear() needed.
            p.slot_list.viewport_offset = 0;
            p.slot_list.selected_indices.clear();
            p.slot_list.selected_offset = None;
            app.library.songs.clear();
        }
    }
    app.pending_expand_center_only = false;
    app.pending_expand = Some(pending);
}

pub(crate) fn pending_expand_timeout_task(pending: PendingExpand) -> Task<Message> {
    use std::time::Duration;
    let label_msg = match &pending {
        PendingExpand::Album { album_id, .. } => Message::PendingExpandAlbumTimeout(album_id.clone()),
        PendingExpand::Artist { artist_id, .. } => Message::PendingExpandArtistTimeout(artist_id.clone()),
        PendingExpand::Genre { genre_id, .. } => Message::PendingExpandGenreTimeout(genre_id.clone()),
        PendingExpand::Song { song_id, .. } => Message::PendingExpandSongTimeout(song_id.clone()),
    };
    Task::perform(async { tokio::time::sleep(Duration::from_millis(2000)).await; }, move |_| label_msg)
}
```

The `handle_pending_expand_*_timeout` and `handle_navigate_and_expand_*` / `handle_browser_pane_navigate_and_expand_*` functions keep their names but become 5-line wrappers calling these helpers. Callers in `main.rs`, `update/albums.rs`, `update/artists.rs`, `update/songs.rs`, `update/genres.rs` stay byte-identical.

The four `expand_*_timeout_task` free fns at the top of `navigation.rs:19-58` are deleted; the lone caller is `pending_expand_timeout_task`.

### 2.3 `for_each_expandable_entity!` test macro — closes the test mirror

**Location**: new module `src/update/tests/navigation_macros.rs`, or inline at the top of `tests/navigation.rs` if the implementer prefers a single file.

Per `~/nokkvi-audit-results/dry-tests.md` §4. Each scenario kernel becomes a macro that takes entity tokens and expands once per row:

```rust
// In tests/navigation_macros.rs (or inline)
macro_rules! for_each_expandable_entity {
    ($mac:ident) => {
        $mac!(album,
            factory:             make_album,
            page_field:          albums_page,
            library_field:       albums,
            pending_var:         crate::state::PendingExpand::Album,
            pending_field:       album_id,
            pin_var:             crate::state::PendingTopPin::Album,
            view_const:          crate::View::Albums,
            page_message:        crate::views::AlbumsMessage,
            children_loaded_msg: TracksLoaded,
            handle_view_fn:      handle_albums,
            try_resolve_fn:      try_resolve_pending_expand_album,
            handle_navigate_fn:  handle_navigate_and_expand_album,
            handle_browser_fn:   handle_browser_pane_navigate_and_expand_album,
            handle_timeout_fn:   handle_pending_expand_album_timeout,
        );
        $mac!(artist, /* … artists_page, ArtistsMessage, AlbumsLoaded, … */);
        $mac!(genre,  /* … genres_page, GenresMessage, AlbumsLoaded, … */);
    };
}

macro_rules! find_chain_scenarios {
    ($name:ident, factory: $factory:ident, page_field: $page:ident, library_field: $lib:ident,
     pending_var: $pending:path, pending_field: $pfield:ident, pin_var: $pin:path,
     view_const: $view:expr, page_message: $msg:path,
     children_loaded_msg: $children_msg:ident, handle_view_fn: $handle_view:ident,
     try_resolve_fn: $resolve:ident, handle_navigate_fn: $navigate:ident,
     handle_browser_fn: $browser:ident, handle_timeout_fn: $timeout:ident,
    ) => {
        mod $name {
            use super::*;

            #[test]
            fn navigate_and_expand_clears_search_filter_and_sets_target() { /* kernel */ }

            #[test]
            fn navigate_and_expand_collapses_existing_expansion() { /* kernel */ }

            #[test]
            fn browser_pane_navigate_and_expand_sets_browsing_flag() { /* kernel */ }

            #[test]
            fn pending_target_cleared_on_switch_view_away() { /* kernel */ }

            // … 13 more scenario kernels mirroring the tri-mirror table from
            //     dry-tests.md §4
        }
    };
}

for_each_expandable_entity!(find_chain_scenarios);
```

Test names stay searchable: `cargo test album::pending_target_cleared_on_switch_view_away` works exactly as before (just under the `album::` module prefix).

**Out-of-scope rows** (kept as bespoke standalone tests, not in the macro):
- `try_resolve_pending_expand_genre_matches_by_name_not_internal_id` — genre's quirk; keep prose.
- `try_resolve_pending_expand_genre_clears_when_idle_and_missing` — genre's single-shot variant; keep prose.
- `try_resolve_pending_expand_album_center_only_centers_without_top_pin` — album-only test of the center-only branch.
- All Song-targeting tests (`try_resolve_pending_expand_song_*`, `start_center_on_playing_song_chain_*`) — songs are center-only / not expandable; the kernel doesn't fit. Keep as-is.
- All `*_focus_and_expand_triggers_*_load` artwork-prefetch tests — entity-quirky (collage vs large-artwork), keep prose.
- All `*_shift_enter_*` and `genres_context_menu_*` tests — view-specific message routing, not chain.
- All `*_sort_mode_most_played_*` and `*_navigate_and_filter_*` tests — adjacent but not part of the chain mirror.

**Expected reduction**: per `dry-tests.md §4`, ~1820 LOC of mirror → ~665 LOC of macro + kernels (incl. macro defs). Net file size drops from 2037 to ~700 LOC. Test count drops from 96 surfaced names to ~50, but **every `#[test]` body remains a real `#[test]` after macro expansion** — coverage unchanged, only scroll cost shrinks.

**Small helper additions** (per `dry-tests.md §3`):
- `arm_pending_album/artist/genre/song(app, id)` — collapses the 38 `app.pending_expand = Some(PendingExpand::X { id, for_browsing_pane: false })` sites.
- `albums_indexed/artists_indexed/genres_indexed/songs_indexed(n)` — replaces the 11 `(0..N).map(|i| make_X(...))` bulk fixtures.
- `seed_albums/artists/genres/songs(app, items)` — sugar over `set_from_vec`; thin but pairs with the macro for consistency.
- `expand_albums_with(app, id, children)` family — replaces 14 paired `expanded_id = Some(...) + children = vec![...]` blocks.

Land these helpers as part of Lane C; they're called from inside the macro kernel.

---

## 3. Lane decomposition (parallel)

Three independent lanes, no required ordering:

| Lane | Scope | Files touched | Commit count (est.) | Effort |
|---|---|---|---:|---|
| **A** (resolver consolidation) | `try_resolve_pending_expand_*` × 4 → `ResolveSpec` trait + 4 thin wrappers | `src/update/navigation.rs` (resolver section, ~640-1024); maybe new `src/update/pending_expand_resolve.rs` | 3-5 | M-L |
| **B** (priming + timeouts + handlers) | `prime_expand_*_target` × 4, `handle_pending_expand_*_timeout` × 4, `expand_*_timeout_task` × 4, `handle_navigate_and_expand_*` × 4, `handle_browser_pane_navigate_and_expand_*` × 4 → free helpers + thin wrappers | `src/update/navigation.rs` (top + priming + timeout sections, ~19-58, ~509-611, ~711-762, ~944-975) | 3-5 | M |
| **C** (test mirror dedup) | `for_each_expandable_entity!` macro + scenario kernels + small helpers | `src/update/tests/navigation.rs`, `src/test_helpers.rs`, optionally new `src/update/tests/navigation_macros.rs` | 4-7 | L |

**Conflict zones**:
- Lane A and Lane B both touch `src/update/navigation.rs` but at **disjoint line ranges**. Lane A owns the resolver section (`:843-1024` plus `:640-705`, `:769-836`); Lane B owns the priming/handler section (`:19-58`, `:509-611`, `:711-762`, `:944-975`, plus the four `handle_navigate_and_expand_*` / `handle_browser_pane_*` blocks at `:509-538`, `:572-594`, `:711-734`, plus the matching `start_center_on_playing_*` blocks at `:1027-1078`). Whichever lane merges first, the other rebases mechanically — line numbers shift, but the regions don't overlap. **Recommended merge order: A → B → C**, but A↔B can run either order with a small rebase.
- Lane A and Lane B might both want a new shared module (`src/update/pending_expand.rs`). To avoid file-creation conflicts: **Lane A creates `src/update/pending_expand_resolve.rs` (resolver-only)**; **Lane B keeps its helpers inside `update/navigation.rs` itself** (or creates `src/update/pending_expand_prime.rs` if cleaner). A follow-up consolidation can fold both into one module post-merge.
- Lane C is in different files (`tests/navigation.rs` + `test_helpers.rs`) and depends only on the public function names (`try_resolve_pending_expand_*`, `handle_navigate_and_expand_*`, `handle_pending_expand_*_timeout`), which all three lanes preserve.

---

## 4. Per-lane scope (callers verified at baseline `c45258b`)

### Lane A — resolver consolidation

**Files**:
- `src/update/navigation.rs` — resolver section.
- New `src/update/pending_expand_resolve.rs` (recommended) — trait + 4 zero-sized impls. Wire into `update/mod.rs` with `mod pending_expand_resolve;`.

**Sites to migrate**:
- `src/update/navigation.rs:640-705` — `try_resolve_pending_expand_genre`. Becomes 1-line `self.try_resolve_pending_expand_with::<GenreSpec>()`.
- `src/update/navigation.rs:769-836` — `try_resolve_pending_expand_artist`.
- `src/update/navigation.rs:843-925` — `try_resolve_pending_expand_album`.
- `src/update/navigation.rs:977-1021` — `try_resolve_pending_expand_song`.

**Spec implementations**:
- `AlbumSpec`: `match_target` uses `item.id == target ? Cow::Borrowed(target) : None`; `focus_and_expand` returns `Some(Message::Albums(views::AlbumsMessage::FocusAndExpand(idx)))`; `pin` returns `Some(PendingTopPin::Album(id))`; `force_load_next_page` returns `Some(self.force_load_albums_page(offset))`; label `"Album"`.
- `ArtistSpec`: same shape, `library.artists`, `Artists(FocusAndExpand)`, pin `PendingTopPin::Artist`, `force_load_artists_page`, label `"Artist"`.
- `GenreSpec`: `match_target` does `if item.name == target { Some(Cow::Owned(item.id.clone())) } else { None }` — name-match returning resolved internal id; `focus_and_expand` returns `Some(Message::Genres(views::GenresMessage::FocusAndExpand(idx)))`; `pin` returns `Some(PendingTopPin::Genre(resolved))`; `force_load_next_page` returns `None` (single-shot); label `"Genre"`.
- `SongSpec`: `match_target` matches by id; `focus_and_expand` returns `None` (center-only always); `pin` returns `None`; `force_load_next_page` returns `Some(self.force_load_songs_page(offset))`; label `"Song"`.

**External callers preserved**: `update/albums.rs:151,254`, `update/artists.rs:229,311`, `update/genres.rs:156`, `update/songs.rs:176,285`, `tests/navigation.rs` (58 sites). All call `app.try_resolve_pending_expand_X()` by their existing name; the 4 wrappers preserve the API.

**Note on the existing differences**: the four resolver bodies today have minor wording variations in their `debug!` / `warn!` / toast strings ("Album not found in library" vs "Genre not found in library"). The migration normalizes these via `S::label()` — that's intentional; the audit flagged the prose variation as drift hazard, not a feature.

### Lane B — priming, timeouts, handlers

**Files**:
- `src/update/navigation.rs` — priming + timeout + handler sections.
- Optional: `src/update/pending_expand_prime.rs` for the helpers — implementer's call.

**Sites to migrate**:
- `src/update/navigation.rs:19-58` — delete the four `expand_*_timeout_task` free fns; replace with one `pending_expand_timeout_task(pending: PendingExpand)`.
- `src/update/navigation.rs:539-562` — `prime_expand_album_target` body → call `prime_expand_target(self, PendingExpand::Album { .. })`.
- `src/update/navigation.rs:595-609` — `prime_expand_genre_target` → same.
- `src/update/navigation.rs:734-748` — `prime_expand_artist_target` → same.
- `src/update/navigation.rs:948-962` — `prime_expand_song_target` → same.
- `src/update/navigation.rs:509-518` — `handle_navigate_and_expand_album`: now calls `prime_expand_target` + `handle_switch_view(View::Albums)` + `pending_expand_timeout_task(PendingExpand::Album { … })`.
- Same for `handle_navigate_and_expand_artist/genre` and the 3 `handle_browser_pane_navigate_and_expand_*` (`:520-537`, `:579-593`, `:718-732`).
- `src/update/navigation.rs:612-624` — `handle_pending_expand_genre_timeout` → wrapper calling shared timeout body.
- Same for `handle_pending_expand_artist_timeout` (`:751-762`), `handle_pending_expand_album_timeout` (`:931-942`), `handle_pending_expand_song_timeout` (`:964-975`).
- `src/update/navigation.rs:1027-1078` — `start_center_on_playing_*_chain` × 4: each calls the corresponding `prime_expand_*_target` + sets `pending_expand_center_only = true`. Migration: each becomes `prime_expand_target(self, PendingExpand::X { … })` + `self.pending_expand_center_only = true` + the existing `handle_switch_view(...)` + load dispatch — keep the `pending_expand_center_only = true` write *after* the `prime` call (since `prime_expand_target` resets `center_only` to false unconditionally; this order matches the current behavior where `start_center_on_playing_*_chain` resets in `prime_expand_*_target` then re-arms).

**External callers preserved**:
- `main.rs:273-284` — comments only; no code.
- `update/{albums,artists,genres,songs}.rs:131,151,254,229,311,156,176,285` — only call `try_resolve_pending_expand_*` (Lane A).
- `update/components.rs`, `update/hotkeys/navigation.rs`, `update/cross_pane_drag.rs` — call `cancel_pending_expand`, `start_center_on_playing_*_chain`, `handle_navigate_and_expand_*`, `handle_browser_pane_navigate_and_expand_*`. All names preserved.

### Lane C — test mirror dedup

**Files**:
- `src/update/tests/navigation.rs` — bulk migration target.
- `src/test_helpers.rs` — small helpers added.
- Optional new `src/update/tests/navigation_macros.rs` (declared `#[cfg(test)] mod navigation_macros;` in `update/tests/mod.rs`).

**Sites to migrate** (per `dry-tests.md §4` table):
- `tests/navigation.rs:315-642` — Album expand chain (14 tests). Migrate all into `mod album { … }` macro instantiation, except: `try_resolve_pending_expand_album_center_only_centers_without_top_pin` (line 1923, album-only branch) stays bespoke.
- `tests/navigation.rs:658-967` — Artist expand chain (15 tests). Migrate to `mod artist { … }`.
- `tests/navigation.rs:1142-1474` — Genre expand chain (15 tests). Migrate to `mod genre { … }` EXCEPT:
  - `try_resolve_pending_expand_genre_matches_by_name_not_internal_id` (`:1246`) — keep bespoke.
  - `try_resolve_pending_expand_genre_clears_when_idle_and_missing` (`:1348`) — keep bespoke (single-shot quirk).
- `tests/navigation.rs:1903-2036` — Song chain (5 tests). **Keep entirely outside the macro** — songs don't fit the album/artist/genre kernel (no FocusAndExpand, no expansion).
- `tests/navigation.rs:968-1141` — re-pin tests, retain (per `dry-tests.md §4` "the entity-quirk tests stay outside the macro for failure isolation").
- `tests/navigation.rs:1758-1819` — focus-and-expand artwork tests, retain.
- `tests/navigation.rs:1607-1758` — shift-enter tests, retain.
- `tests/navigation.rs:1540-1606` — genre context-menu tests, retain.
- `tests/navigation.rs:1474-1540` — sort-mode tests, retain.

**Helpers to add** (in `src/test_helpers.rs`):
```rust
pub(crate) fn arm_pending_album(app: &mut Nokkvi, id: &str) { /* … */ }
pub(crate) fn arm_pending_artist(app: &mut Nokkvi, id: &str) { /* … */ }
pub(crate) fn arm_pending_genre(app: &mut Nokkvi, id: &str) { /* … */ }
pub(crate) fn arm_pending_song(app: &mut Nokkvi, id: &str) { /* … */ }
pub(crate) fn albums_indexed(n: usize) -> Vec<AlbumUIViewData> { /* … */ }
pub(crate) fn artists_indexed(n: usize) -> Vec<ArtistUIViewData> { /* … */ }
pub(crate) fn genres_indexed(n: usize) -> Vec<GenreUIViewData> { /* … */ }
pub(crate) fn songs_indexed(n: usize) -> Vec<SongUIViewData> { /* … */ }
pub(crate) fn seed_albums(app: &mut Nokkvi, items: Vec<AlbumUIViewData>) { /* … */ }
pub(crate) fn seed_artists(app: &mut Nokkvi, items: Vec<ArtistUIViewData>) { /* … */ }
pub(crate) fn seed_genres(app: &mut Nokkvi, items: Vec<GenreUIViewData>) { /* … */ }
pub(crate) fn seed_songs(app: &mut Nokkvi, items: Vec<SongUIViewData>) { /* … */ }
```

The 4 `expand_*_with` helpers from `dry-tests.md §3.5` are nice-to-have but only used in 14 sites — defer to a follow-up PR if Lane C lands big enough. Implementer's discretion.

---

## 5. Verification (every lane)

Run after each commit slice:

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass before pushing the slice. Per-lane TDD is light because the changes are structural (no behavior changes — every test that passed before must still pass).

**Lane A specific check**: after migration, the four resolver wrappers should each be ≤5 lines. `grep -c 'try_resolve_pending_expand_with' src/update/navigation.rs` should equal `4` (one per wrapper). The pre-migration warn/toast strings get normalized — confirm by searching for the legacy phrasings (`"Album not found in library"` etc.) in source: should be zero (the format string `"{} not found in library"` is the new shape).

**Lane B specific check**: the four `expand_*_timeout_task` free fns are gone. `grep -c 'fn expand_.*_timeout_task' src/update/navigation.rs` should equal `0`. The four `prime_expand_*_target` are still present BUT each body is ≤4 lines (the wrapper). `grep -c 'fn prime_expand_' src/update/navigation.rs` stays `4`.

**Lane C specific check**: `cargo test --lib navigation::album` and `::artist` / `::genre` should each list ~13-17 tests in their submodule. Total navigation test count drops from 96 to roughly 50-55 (depending on bespoke retention). Coverage assertion: every test name from the pre-migration baseline either survives verbatim outside the macro OR has a matching `<entity>::<scenario>` form post-migration.

---

## 6. What each lane does NOT do

- **No public API rename.** `try_resolve_pending_expand_album/artist/genre/song`, `handle_navigate_and_expand_*`, `handle_browser_pane_navigate_and_expand_*`, `handle_pending_expand_*_timeout`, `prime_expand_*_target`, `cancel_pending_expand`, `start_center_on_playing_*_chain` keep their names. They become wrappers / 5-LOC factory calls. UI callers stay untouched.
- **No new dependency.** Plan uses zero-sized types, trait dispatch, and `Cow` from std — all already in use.
- **No behavior change.** Every existing test must pass. Toast / warn / debug strings are normalized via `S::label()`, but the message *content* is preserved (label noun stays "Album"/"Artist"/"Genre"/"Song"; the format prefix doesn't change semantics).
- **No fold of the genre name-vs-id quirk.** `GenreSpec::match_target` keeps the name-match-to-resolved-id behavior explicitly.
- **No expansion to a 5th entity** (e.g. Playlists / Radios). The plan structurally enables it, but actually adding one is a follow-up.
- **No reformatting outside touched files.**
- **No drive-by docstring rewrites** unrelated to the dedup.
- **No collapse of song-chain tests into the macro.** Songs don't fit the album/artist/genre kernel — keep them prose.
- **No CI grep-test added.** The audit suggests a CI check for `HoverOverlay::new(button(`-style invariants; that's `_SYNTHESIS.md §8 #2` and is out of scope here.
- **No update to `.agent/rules/` files** — Lane C may touch `.agent/audit-progress.md`, but rules-doc syncing is the `/sync-rules` skill's job, not this plan's.

---

## Fanout Prompts

### lane-a-resolver

worktree: ~/nokkvi-pending-expand-a
branch: refactor/pending-expand-resolver
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane A of the pending-expand dedup plan — collapse the 4 `try_resolve_pending_expand_*` resolvers to one descriptor-driven body.

Plan doc: /home/foogs/nokkvi/.agent/plans/pending-expand-dedup.md (sections 2.1, 4 "Lane A").

Working directory: ~/nokkvi-pending-expand-a (this worktree). Branch: refactor/pending-expand-resolver. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` should show `c45258b` or a descendant on `main`.
- `wc -l src/update/navigation.rs` should report 1139.
- `grep -n 'fn try_resolve_pending_expand_' src/update/navigation.rs` should list 4 fns at lines 640 (genre), 769 (artist), 843 (album), 977 (song).
- `grep -rn 'try_resolve_pending_expand_' src/ --include='*.rs' | grep -v 'tests/navigation.rs\|update/navigation.rs' | wc -l` should report 7 external call sites.

### 2. Add the trait module

Create `src/update/pending_expand_resolve.rs`. Declare in `src/update/mod.rs` (alphabetical placement):
```rust
mod pending_expand_resolve;
pub(crate) use pending_expand_resolve::{AlbumSpec, ArtistSpec, GenreSpec, SongSpec};
```

The module contains:
- `pub(crate) trait ResolveSpec` per plan §2.1.
- `pub(crate) struct AlbumSpec; ArtistSpec; GenreSpec; SongSpec;` (zero-sized).
- `impl ResolveSpec for AlbumSpec/ArtistSpec/GenreSpec/SongSpec`.

The generic `try_resolve_pending_expand_with::<S>` method goes on `impl Nokkvi` in the same module (so it has access to `Nokkvi`'s field structure).

### 3. Implement the four specs

For each spec:

**AlbumSpec**: 
- `Item = AlbumUIViewData`
- `target_id`: matches `PendingExpand::Album { album_id, .. }`.
- `library`: returns `&library.albums`.
- `page_mut`: returns `&mut app.albums_page.common.slot_list`.
- `match_target`: `if item.id == target { Some(Cow::Borrowed(target)) } else { None }`.
- `focus_and_expand(idx)`: `Some(Message::Albums(views::AlbumsMessage::FocusAndExpand(idx)))`.
- `pin(id)`: `Some(PendingTopPin::Album(id))`.
- `force_load_next_page(app, offset)`: `Some(app.force_load_albums_page(offset))`.
- `label()`: `"Album"`.

**ArtistSpec**: `library.artists`, `artists_page`, `Artists(FocusAndExpand)`, `Artist(id)`, `force_load_artists_page`, `"Artist"`.

**GenreSpec**: 
- `match_target`: `if item.name == target { Some(Cow::Owned(item.id.clone())) } else { None }` — name-match returning resolved internal id.
- `force_load_next_page`: `None` (single-shot).
- Other methods: `genres_page`, `Genres(FocusAndExpand)`, `Genre(resolved_id)`, `"Genre"`.

**SongSpec**: 
- `match_target`: id-match like Album.
- `focus_and_expand`: `None` (songs aren't expandable; resolver only centers).
- `pin`: `None`.
- `force_load_next_page`: `Some(app.force_load_songs_page(offset))`.
- `library.songs`, `songs_page`, `"Song"`.

### 4. Implement the generic resolver body

Per plan §2.1's pseudocode. Key steps:

1. Pull `target_id` from `self.pending_expand` via `S::target_id`. Early-return `None` if absent.
2. Scan `S::library(&self.library)` with `iter().enumerate().find_map(|(i, item)| S::match_target(item, &target_id).map(|r| (i, r.into_owned())))`.
3. **Found branch**: 
   - Snapshot `center_only`, clear `pending_expand` and `pending_expand_center_only`.
   - Call `S::page_mut(self).set_offset(target_offset, total)`, `pin_selected(idx, total)`, `flash_center()`.
   - `let prefetch_task = self.prefetch_viewport_artwork();`
   - If `center_only`: return `Some(prefetch_task)`.
   - Else: `self.pending_top_pin = S::pin(resolved_id);` (None for songs is fine — overwrites with None).
   - Return `Some(match S::focus_and_expand(idx) { Some(msg) => Task::batch([prefetch_task, Task::done(msg)]), None => prefetch_task })`.
4. **Not-found branches**:
   - If `library.fully_loaded()`: warn + toast `"{label} not found in library"`, clear pending state, return `Some(Task::none())`.
   - If `library.is_loading()`: return `None` (wait for next page).
   - Else (idle, not-found, more pages possible): try `S::force_load_next_page(self, library.loaded_count())`; if `None` (single-shot Genre), warn + toast + return `Some(Task::none())`; if `Some(task)` return it.

Match the existing log strings' *information content* (entity label + target id + index) but normalize prefix/suffix prose via `S::label()`. The `pre-resolved-id` debug log on Genre (`"... (id={resolved_id})"`) is an implementation detail of the genre spec — drop it from the generic body, since it's redundant with the `pending_top_pin = Some(...)` write that's visible in the trace.

### 5. Replace the four resolver bodies in navigation.rs

Each body becomes a 1-liner wrapper:

```rust
pub(crate) fn try_resolve_pending_expand_album(&mut self) -> Option<Task<Message>> {
    self.try_resolve_pending_expand_with::<crate::update::AlbumSpec>()
}
```

Same for artist, genre, song. Delete the 70-90 LOC bodies the wrappers replace.

**Preserve the doc comments** above each fn — they describe the per-entity quirks (genre's name-vs-id, song's center-only, album's pagination) which are still load-bearing context for future readers, even if the body delegates.

### 6. Verify

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass. The 23 navigation tests touching `try_resolve_pending_expand_*` (counted at baseline by `grep -c 'try_resolve_pending_expand_' src/update/tests/navigation.rs`) must pass without modification.

### 7. Commit slices

Commit each verified slice without pausing — this is a feature branch in a worktree per global feedback:

1. `refactor(navigation): add ResolveSpec trait for pending-expand resolver` — module + trait + 4 zero-sized impls (no migration yet; the generic body lands here).
2. `refactor(navigation): migrate try_resolve_pending_expand_album to ResolveSpec` — wrapper rewrite.
3. `refactor(navigation): migrate try_resolve_pending_expand_artist to ResolveSpec`.
4. `refactor(navigation): migrate try_resolve_pending_expand_genre to ResolveSpec` — note GenreSpec's name-match quirk in the commit body.
5. `refactor(navigation): migrate try_resolve_pending_expand_song to ResolveSpec` — note SongSpec's center-only / no-FocusAndExpand quirk.

Each slice runs the four-step verify. Skip the `Co-Authored-By` trailer per global instructions.

### 8. Update audit tracker

After the final commit, append commit refs to `.agent/audit-progress.md` §7 row 6 (the "update/navigation.rs pending-expand dedup" row) and §3 row 1. **Do not mark the row done** unless Lane B has also landed — note "Lane A (resolvers) complete; Lane B (priming/handlers) pending".

## What NOT to touch

- Anything in the priming/timeout/handler section of `update/navigation.rs` (lines 19-58, 509-611, 711-762, 944-975) — Lane B's territory.
- Any `*_page` field other than reading via `S::page_mut`.
- The UI crate beyond what `try_resolve_pending_expand_*` currently touches.
- `src/update/tests/navigation.rs` — Lane C migrates tests; you don't.
- `.agent/rules/` files.

## If blocked

- If `cargo test` fails with an unrelated test failure on baseline: stop, report, do not proceed.
- If a `try_resolve_pending_expand_*` caller exists that you didn't expect (anything outside the 7 sites listed): stop, list it, ask.
- If clippy flags the trait dispatch (e.g., `unused_self` on a wrapper): adjust minimally; do not paper over with `#[allow]`.
- If the resolver wrappers can't return a `Task<Message>` because the trait method's lifetime won't compose: lift the offending part into a free fn taking `&mut Nokkvi` rather than a method.

## Reporting

End with: commits (refs + subjects), `wc -l src/update/navigation.rs` delta, `grep -c 'try_resolve_pending_expand_with' src/update/navigation.rs` final value (should equal `4`), test count delta (should be `0` — all tests preserved).
````

### lane-b-priming

worktree: ~/nokkvi-pending-expand-b
branch: refactor/pending-expand-priming
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane B of the pending-expand dedup plan — collapse `prime_expand_*_target`, `handle_pending_expand_*_timeout`, `expand_*_timeout_task`, and the navigate-and-expand handler shells (top-pane + browsing-pane) to descriptor-driven shapes.

Plan doc: /home/foogs/nokkvi/.agent/plans/pending-expand-dedup.md (sections 2.2, 4 "Lane B").

Working directory: ~/nokkvi-pending-expand-b (this worktree). Branch: refactor/pending-expand-priming. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `c45258b` or a descendant on `main`.
- `grep -n 'fn expand_.*_timeout_task\|fn prime_expand_\|fn handle_pending_expand_.*_timeout\|fn handle_navigate_and_expand_\|fn handle_browser_pane_navigate_and_expand_' src/update/navigation.rs` should list:
  - 4 `expand_*_timeout_task` free fns at lines 19, 29, 39, 50.
  - 4 `prime_expand_*_target` methods at lines 539, 595, 734, 948.
  - 4 `handle_pending_expand_*_timeout` methods at lines 612, 751, 931, 964.
  - 4 `handle_navigate_and_expand_*` methods at lines 509, 572, 711, plus the absent Song which uses only the chain entry point (verify).
  - 3 `handle_browser_pane_navigate_and_expand_*` methods at lines 520, 579, 718.

### 2. Add the priming + timeout helpers

Either inline at the top of `update/navigation.rs` (under a section banner `// === pending-expand helpers ===`), OR create `src/update/pending_expand_prime.rs` declared `pub(crate) mod pending_expand_prime;` in `update/mod.rs`. Implementer's call.

Implement per plan §2.2:

```rust
pub(crate) fn prime_expand_target(app: &mut Nokkvi, pending: PendingExpand) {
    match &pending {
        PendingExpand::Album { .. } => { /* per plan §2.2 — albums_page + library.albums + expansion.clear() */ }
        PendingExpand::Artist { .. } => { /* artists_page + library.artists + expansion.clear() */ }
        PendingExpand::Genre { .. } => { /* genres_page + library.genres + expansion.clear() */ }
        PendingExpand::Song { .. } => { /* songs_page + library.songs — NO expansion.clear() (songs aren't expandable) */ }
    }
    app.pending_expand_center_only = false;
    app.pending_expand = Some(pending);
}

pub(crate) fn pending_expand_timeout_task(pending: PendingExpand) -> Task<Message> {
    use std::time::Duration;
    let timeout_msg = match &pending {
        PendingExpand::Album { album_id, .. } => Message::PendingExpandAlbumTimeout(album_id.clone()),
        PendingExpand::Artist { artist_id, .. } => Message::PendingExpandArtistTimeout(artist_id.clone()),
        PendingExpand::Genre { genre_id, .. } => Message::PendingExpandGenreTimeout(genre_id.clone()),
        PendingExpand::Song { song_id, .. } => Message::PendingExpandSongTimeout(song_id.clone()),
    };
    Task::perform(
        async { tokio::time::sleep(Duration::from_millis(2000)).await; },
        move |_| timeout_msg,
    )
}
```

Verify the four `Message::PendingExpand*Timeout(...)` variants exist at baseline: `grep -n 'PendingExpand.*Timeout' src/app_message.rs`. If a variant is missing or shaped differently, STOP and report — the plan's assumption is wrong.

### 3. Migrate `prime_expand_*_target` to delegate

Each method becomes a 3-line wrapper:

```rust
fn prime_expand_album_target(&mut self, album_id: String, for_browsing_pane: bool) {
    prime_expand_target(self, PendingExpand::Album { album_id, for_browsing_pane });
}
```

Same shape for artist / genre / song. Delete the 8-15 LOC bodies the wrappers replace.

**Note on song**: the existing `prime_expand_song_target` body has a comment about songs not being expandable and skipping `expansion.clear()`. The dispatch table in `prime_expand_target`'s `PendingExpand::Song` arm preserves this. Keep the doc comment on `prime_expand_song_target` itself — readers benefit from the per-method note.

### 4. Migrate `handle_pending_expand_*_timeout` to delegate

```rust
pub(crate) fn handle_pending_expand_album_timeout(&mut self, album_id: String) -> Task<Message> {
    if matches!(
        &self.pending_expand,
        Some(PendingExpand::Album { album_id: pending, .. }) if pending == &album_id
    ) {
        self.toast_info("Finding album…");
    }
    Task::none()
}
```

The 4 timeout methods are nearly identical apart from the `PendingExpand::X { x_id, .. }` and toast text. They could collapse to one helper with a `match` over the PendingExpand variant for the toast label, **but** the 4 methods are dispatched from 4 different `Message::*` variants, so callers stay 4. Two reasonable shapes:

a. Keep 4 methods, each ~5 lines (match against PendingExpand variant + toast).
b. Add a free `pending_expand_timeout_toast(app, expected: PendingExpand)` helper that does the matches + toast, called by all 4 methods.

Pick (b) if Clippy doesn't complain; otherwise (a) is fine. Either way, the 4 methods stay reachable by name from the dispatcher in `update/mod.rs`.

### 5. Delete the 4 `expand_*_timeout_task` free functions

Lines 19-58 in baseline. Replace their callers (3 in `handle_navigate_and_expand_*`, 3 in `handle_browser_pane_navigate_and_expand_*`, possibly more in `start_center_on_playing_*_chain`) with `pending_expand_timeout_task(PendingExpand::X { ... })`.

**Constructor mismatch warning**: the existing free fns take just `id: String`. The new helper takes `PendingExpand` (with `id` and `for_browsing_pane`). Each call site has both pieces of data already — pass them explicitly. Example:

```rust
// Before:
expand_album_timeout_task(album_id.clone())

// After:
pending_expand_timeout_task(PendingExpand::Album { album_id: album_id.clone(), for_browsing_pane: false })
```

Note `for_browsing_pane` doesn't affect the timeout body — the helper only uses the id field. But passing the right value keeps the type honest.

### 6. Migrate `handle_navigate_and_expand_*` and `handle_browser_pane_navigate_and_expand_*`

Each becomes a 5-line wrapper:

```rust
pub(crate) fn handle_navigate_and_expand_album(&mut self, album_id: String) -> Task<Message> {
    self.prime_expand_album_target(album_id.clone(), false);
    let switch_task = self.handle_switch_view(View::Albums);
    Task::batch([
        switch_task,
        pending_expand_timeout_task(PendingExpand::Album { album_id, for_browsing_pane: false }),
    ])
}
```

The browsing-pane variant uses `BrowsingView::X` + `Message::LoadX` per the existing body shape. Don't rewrite the load-task; preserve it verbatim.

### 7. Migrate `start_center_on_playing_*_chain` to use the new primer

Each chain entry point (`navigation.rs:1027-1078`) currently calls `prime_expand_*_target(id, false)` then sets `pending_expand_center_only = true`. The `prime_expand_target` helper resets `center_only = false` unconditionally (matches today's behavior). Keep the *order* unchanged: `prime_expand_*_target(...)` first, then `self.pending_expand_center_only = true` second. Don't try to fold `center_only` into the helper — it's a separate concern and folding it would change observable behavior.

### 8. Verify

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass. The 35 navigation tests touching `handle_navigate_and_expand_*` / `handle_pending_expand_*_timeout` / `prime_expand_*_target` (counted at baseline by `grep -c 'handle_navigate_and_expand_\|handle_pending_expand_.*_timeout\|prime_expand_' src/update/tests/navigation.rs`) must pass without modification.

### 9. Commit slices

1. `refactor(navigation): add prime_expand_target + pending_expand_timeout_task helpers` — new helpers, no migration.
2. `refactor(navigation): migrate prime_expand_*_target to shared helper` — 4 method bodies become wrappers; doc comments preserved.
3. `refactor(navigation): replace expand_*_timeout_task free fns with pending_expand_timeout_task` — deletes 40 LOC of mirror.
4. `refactor(navigation): migrate handle_pending_expand_*_timeout to shared body` — toast routing collapses.
5. `refactor(navigation): migrate handle_navigate_and_expand_* + browsing-pane variants` — handler shells.
6. `refactor(navigation): migrate start_center_on_playing_*_chain to shared primer` — preserves center_only ordering.

Each slice: `cargo test && cargo clippy --all-targets -- -D warnings && cargo +nightly fmt --all -- --check`. Skip `Co-Authored-By` trailer.

### 10. Update audit tracker

After the final commit, append commit refs to `.agent/audit-progress.md` §7 row 6 + §3 row 1. **Mark the row ✅ done ONLY if Lane A has also landed**; otherwise leave 🟡 partial with note "Lane B (priming/handlers) complete; Lane A (resolvers) pending".

## What NOT to touch

- The four `try_resolve_pending_expand_*` resolver bodies (lines 640-1024) — Lane A's territory.
- `tests/navigation.rs` — Lane C's territory.
- The UI crate (`src/views/`, `src/widgets/`) beyond what's needed.
- `.agent/rules/` files.
- The 4 `Message::PendingExpand*Timeout(id)` variants in `app_message.rs` — preserve verbatim (the timeout helper consumes the existing variants).

## If blocked

- If a `Message::PendingExpand*Timeout` variant is shaped differently than expected: STOP, report.
- If `start_center_on_playing_*_chain` has order-sensitive logic the plan didn't anticipate: report what changes after the migration and confirm before committing.
- If the test suite reveals a `pending_expand_center_only` regression: that's a real surprise — the helper should preserve order. Investigate before adjusting.

## Reporting

End with: commits (refs + subjects), `wc -l src/update/navigation.rs` delta (expected ~150-200 LOC reduction), `grep -c 'fn expand_.*_timeout_task' src/update/navigation.rs` final value (should be `0`), test count delta (should be `0`).
````

### lane-c-tests

worktree: ~/nokkvi-pending-expand-c
branch: refactor/pending-expand-tests
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane C of the pending-expand dedup plan — collapse the album/artist/genre tri-mirror in `tests/navigation.rs` to a `for_each_expandable_entity!` macro; add small fixture helpers to `test_helpers.rs`.

Plan doc: /home/foogs/nokkvi/.agent/plans/pending-expand-dedup.md (sections 2.3, 4 "Lane C"). Source for the macro pattern: `~/nokkvi-audit-results/dry-tests.md` §4.

Working directory: ~/nokkvi-pending-expand-c (this worktree). Branch: refactor/pending-expand-tests. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `c45258b` or a descendant on `main`.
- `wc -l src/update/tests/navigation.rs` reports 2037.
- `grep -c '^#\[test\]' src/update/tests/navigation.rs` reports 96.
- `grep -c '^#\[test\]' src/test_helpers.rs` reports 0 (helpers only, no tests).

### 2. Add small helpers to `test_helpers.rs`

Per plan §4 "Lane C — Helpers to add". Before adding, grep to confirm the helper name doesn't already exist:

```bash
grep -n 'fn arm_pending_\|fn albums_indexed\|fn seed_albums' src/test_helpers.rs
```

Should return nothing. Add the 12 helpers per plan §4 (4× `arm_pending_*`, 4× `*_indexed(n)`, 4× `seed_*`). Place them after the existing `make_*` factories in alphabetical order.

The `arm_pending_*` helpers MUST set `for_browsing_pane: false` — the only test that arms `for_browsing_pane: true` is `browser_pane_navigate_and_expand_*_sets_browsing_flag` (3 sites in tri-mirror), which calls `handle_browser_pane_navigate_and_expand_*` directly without pre-arming. No test calls a hypothetical `arm_pending_album_browser` — it doesn't exist; don't add it.

### 3. Decide on macro placement

Two options:
- **(A) Inline**: macros + entity-binding go at the top of `tests/navigation.rs` itself, above the first `#[test]`. Simplest if the file ends up around 700 LOC.
- **(B) Sibling module**: create `src/update/tests/navigation_macros.rs` with the entity-binding macro, and import-and-invoke from `tests/navigation.rs`. Cleaner if the macros grow.

Pick (A) unless `navigation.rs` would still be over 1000 LOC after migration; then pick (B). Implementer's call.

### 4. Write the entity-binding macro

```rust
macro_rules! for_each_expandable_entity {
    ($mac:ident) => {
        $mac!(album,
            factory:             make_album,
            indexed_factory:     albums_indexed,
            seed:                seed_albums,
            arm_pending:         arm_pending_album,
            page_field:          albums_page,
            library_field:       albums,
            pending_var:         crate::state::PendingExpand::Album,
            pending_field:       album_id,
            pin_var:             crate::state::PendingTopPin::Album,
            view_const:          crate::View::Albums,
            page_message:        crate::views::AlbumsMessage,
            children_loaded_msg: TracksLoaded,
            handle_view_fn:      handle_albums,
            try_resolve_fn:      try_resolve_pending_expand_album,
            handle_navigate_fn:  handle_navigate_and_expand_album,
            handle_browser_fn:   handle_browser_pane_navigate_and_expand_album,
            handle_timeout_fn:   handle_pending_expand_album_timeout,
            timeout_message_var: crate::Message::PendingExpandAlbumTimeout,
            label_lower:         "album",
        );
        $mac!(artist, /* … artists_page + ArtistUIViewData + ArtistsMessage + AlbumsLoaded + ArtistsAction … */);
        $mac!(genre,  /* … genres_page + GenreUIViewData + GenresMessage + AlbumsLoaded + GenresAction … */);
    };
}
```

Note: **Song is NOT in this macro.** Songs don't fit the album/artist/genre kernel (no FocusAndExpand, no expansion). The 5 song-targeting tests stay bespoke.

### 5. Write the scenario-kernel macros

Each scenario gets its own `macro_rules!` that takes the entity tokens and produces ONE test:

```rust
macro_rules! navigate_and_expand_clears_search_filter_and_sets_target_test {
    ($name:ident, factory: $factory:ident, page_field: $page:ident,
     pending_var: $pending:path, pending_field: $pfield:ident,
     handle_navigate_fn: $navigate:ident, /* … */) => {
        // (the test body below produces one #[test] inside an outer mod $name { … } block.
        // The outer mod is built up by composing all scenario macros — see step 6.)
    };
}
```

There are ~17 scenario kernels per the dry-tests.md §4 table. Some scenarios (`pending_target_cleared_on_switch_view_to_X`) need to know the entity's view const because the assertion changes per-entity. The macro accepts whatever extra tokens it needs.

### 6. Compose the per-entity mod blocks

The cleanest pattern is one mega-macro that takes all the entity tokens and emits a `mod $name { use super::*; <17 #[test] fns> }` block. That way each entity gets one `mod album` / `mod artist` / `mod genre` containing all its mirrored scenarios, and `for_each_expandable_entity!(find_chain_scenarios)` invokes it three times.

```rust
macro_rules! find_chain_scenarios {
    ($name:ident, factory: $factory:ident, /* … all the tokens … */) => {
        mod $name {
            use super::*;

            #[test]
            fn navigate_and_expand_clears_search_filter_and_sets_target() {
                let mut app = test_app();
                app.$page.common.search_query = "rock".to_string();
                app.$page.common.search_input_focused = true;
                let _ = app.$navigate("a1".to_string());
                assert!(app.$page.common.search_query.is_empty());
                assert!(!app.$page.common.search_input_focused);
                match &app.pending_expand {
                    Some($pending { $pfield, for_browsing_pane }) => {
                        assert_eq!($pfield, "a1");
                        assert!(!for_browsing_pane);
                    }
                    other => panic!("expected $pending, got {other:?}"),
                }
            }

            #[test]
            fn navigate_and_expand_collapses_existing_expansion() { /* kernel */ }

            // … 15 more #[test] fns …
        }
    };
}

for_each_expandable_entity!(find_chain_scenarios);
```

Test names stay searchable: `cargo test album::navigate_and_expand_clears_search_filter_and_sets_target` works.

**The 17 scenarios** (see `dry-tests.md §4` for the canonical list):

| # | Scenario kernel | Source line range (album / artist / genre) |
|---|---|---|
| 1 | `navigate_and_expand_clears_search_filter_and_sets_target` | 315-339 / 658-682 / 1142-1170 |
| 2 | `navigate_and_expand_collapses_existing_expansion` | 343-355 / 686-698 / 1175-… |
| 3 | `browser_pane_navigate_and_expand_sets_browsing_flag` | 356-371 / 699-714 / 1175-1200 |
| 4 | `pending_target_cleared_on_switch_view_away` | 372-384 / 715-727 / … |
| 5 | `pending_target_persists_on_switch_view_to_self` | 385-400 / 728-740 / … |
| 6 | `pending_target_cleared_on_navigate_and_filter` | 401-419 / 741-759 / … |
| 7 | `try_resolve_finds_loaded_and_takes_target` | 420-453 / 760-790 / 1284-1318 |
| 8 | `try_resolve_places_target_at_top_slot` | 454-487 / 791-818 / 1319-1347 |
| 9 | `try_resolve_clears_when_fully_loaded_and_missing` | 488-508 / 819-838 / — (variant for genre: clears_when_idle_and_missing — bespoke) |
| 10 | `try_resolve_returns_none_when_loading` | 509-529 / 839-856 / 1371-… |
| 11 | `try_resolve_kicks_next_page_when_idle_and_more_remain` | 530-550 / 857-873 / — (genre is single-shot — skip) |
| 12 | `try_resolve_bypasses_scroll_edge_gate_when_paging` | 551-585 / 874-893 / — (genre is single-shot — skip) |
| 13 | `pending_timeout_does_not_toast_when_target_already_resolved` | 586-598 / 894-901 / 1383-… |
| 14 | `pending_timeout_does_not_toast_for_stale_id` | 599-614 / 902-912 / — (only one in genre block — fold into stale variant or skip) |
| 15 | `pending_timeout_toasts_when_target_still_in_flight` | 615-627 / 913-922 / 1395-… |
| 16 | `try_resolve_sets_top_pin_when_target_found` | 967-988 (album) / 989-1009 (artist) / — (folded into _finds_loaded_ for genre) |
| 17 | `children_loaded_re_pins_selected_offset_for_self` | 1010-1042 / 1043-1075 / 1443-1469 |

Some scenarios are missing from genre (single-shot quirk skips force-load tests, scroll-edge-gate test, stale-id-timeout test). The macro handles this by emitting **fewer tests** for genre — use a `with_pagination` flag in the entity binding:

```rust
$mac!(album, /* … */ has_pagination: true, has_top_pin_test: true);
$mac!(artist, /* … */ has_pagination: true, has_top_pin_test: true);
$mac!(genre, /* … */ has_pagination: false, has_top_pin_test: false);
```

Inside the kernel, `#[cfg(any())]`-style gating doesn't work in declarative macros — instead, use a separate macro per scenario AND a meta-macro that decides which scenarios apply per entity:

```rust
$mac!(album, kernel_set: full);
$mac!(artist, kernel_set: full);
$mac!(genre, kernel_set: single_shot);
```

Then have two top-level orchestrator macros (`find_chain_scenarios_full` and `find_chain_scenarios_single_shot`) that each emit the right subset. This keeps the genre exclusions explicit and the kernels reusable.

### 7. Migrate the bespoke tests (keep prose)

Tests that DO NOT fit the macro stay as-is in `tests/navigation.rs` outside any `mod`:

- `try_resolve_pending_expand_genre_matches_by_name_not_internal_id` (line 1246) — name-vs-id quirk; keep prose.
- `try_resolve_pending_expand_genre_clears_when_idle_and_missing` (line 1348) — single-shot quirk; keep prose.
- `try_resolve_pending_expand_album_center_only_centers_without_top_pin` (line 1923) — center-only branch; keep prose.
- ALL Song-targeting tests (`try_resolve_pending_expand_song_*`, `start_center_on_playing_song_chain_*`) — keep prose.
- ALL re-pin standalone tests (`tracks_loaded_re_pins_selected_offset_for_album`, `albums_loaded_re_pins_selected_offset_for_genre`, etc.) — these are sibling tests to scenario #17 above; if scenario #17 absorbs the album/artist/genre versions, the bespoke ones can be deleted (verify no extra assertions). If they have extra setup (e.g., the misnamed `albums_loaded_re_pins_selected_offset_for_artist` per `_SYNTHESIS.md` B8 — already fixed by `a74d94a`), keep them.
- ALL `*_focus_and_expand_triggers_*_load`, `*_shift_enter_*`, `genres_context_menu_*`, `*_sort_mode_most_played_*`, `*_navigate_and_filter_*` tests — entity-quirky; keep prose.
- `cancel_pending_expand_also_clears_center_only_flag` (line 2023) — covers cancel logic, not the chain; keep prose.

### 8. Apply small-helper migrations to bespoke tests

The 4 `arm_pending_*` and 4 `*_indexed`/`seed_*` helpers should also replace the inline patterns in the BESPOKE tests where applicable (see `dry-tests.md §3.4 — 38 sites for arm_pending`, §3.1 — 11 sites for indexed). Mechanical search/replace; keeps the code consistent across macro-expanded and bespoke tests.

### 9. Verify

```bash
cargo build
cargo test --bin nokkvi 2>&1 | tail -30   # focus on test count
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

Specifically check:
- `cargo test album::` lists ~13-17 tests.
- `cargo test artist::` lists ~13-17 tests.
- `cargo test genre::` lists ~10-12 tests (fewer due to single-shot exclusions).
- Total navigation test count: 96 → ~50-55 (the rest are macro-expanded under `<entity>::`).
- Every test that passed before still passes.

### 10. Commit slices

Commit each verified slice:

1. `test(helpers): add arm_pending / *_indexed / seed_* fixture helpers` — pure additions to `test_helpers.rs`.
2. `test(navigation): add for_each_expandable_entity! macro infrastructure` — entity-binding macro + the `find_chain_scenarios_full` and `_single_shot` orchestrators.
3. `test(navigation): migrate album expand chain to macro` — produces `mod album { … }`; deletes the album test bodies the macro replaces.
4. `test(navigation): migrate artist expand chain to macro` — `mod artist { … }`.
5. `test(navigation): migrate genre expand chain to macro` — `mod genre { … }` with the single-shot kernel set.
6. `test(navigation): apply arm_pending / seed_* helpers to bespoke tests` — mechanical sweep.
7. (optional) `test(navigation): split bespoke tests into navigation/{re_pin,focus_artwork,shift_enter}.rs submodules` — only if `navigation.rs` is still over 1000 LOC after the macro migration.

Each slice runs the four-step verify. Skip the `Co-Authored-By` trailer.

### 11. Update audit tracker

After the final commit, append commit refs to `.agent/audit-progress.md` §7 row 6 + §3 row 1 (mention "test mirror"). Mark §7 #6 fully done ONLY if Lanes A and B have also landed; otherwise leave 🟡 partial with note "Lane C (test dedup) complete; Lanes A/B (source) pending".

## What NOT to touch

- `src/update/navigation.rs` — Lanes A and B's territory.
- The Song-targeting tests (kept bespoke).
- The genre quirk tests (kept bespoke — name-match, single-shot).
- `.agent/rules/` files.
- Any test outside the album/artist/genre tri-mirror.

## If blocked

- If a scenario kernel can't macro-fit cleanly because the assertions actually differ across entities (not just field names): STOP, report which scenario, propose either keeping that scenario bespoke or extending the macro vocabulary.
- If `cargo test` regresses an existing test that the macro was supposed to cover: investigate before adjusting — the kernel might be missing a per-entity nuance.
- If the macro grows over ~400 LOC for the entity-binding alone: consider option (B) (sibling module) for placement.
- If genre's `kernel_set: single_shot` accidentally emits a force-load test (because the meta-macro routing is wrong): fix the routing — never emit a test that calls `force_load_genres_page` (genre is single-shot; the function doesn't exist).

## Reporting

End with: commits (refs + subjects), `wc -l src/update/tests/navigation.rs` delta (expected ~1000-1100 LOC reduction), test count by module (`cargo test --list 2>&1 | grep navigation:: | wc -l` should report the post-migration count), and any scenario that needed non-trivial reshaping (one sentence each).
````
