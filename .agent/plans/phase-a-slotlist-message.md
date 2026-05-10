# Phase A — `SlotListPageMessage` carrier

Closes §4 #9 Phase A: collapse ~60 stamped slot-list navigation variants across 8 per-view
enums into a single shared enum. This is the last sub-phase of the message-architecture plan.

**Prerequisites**: Phase C (`ViewHeaderConfig`) complete ✅. Phase B superseded ✅.

Last verified baseline: **2026-05-10, `main @ HEAD`**.

Research source: live codebase audit 2026-05-10 (see subagent findings).

---

## 1. Problem scope

Every per-view `*Message` enum repeats ~14 navigation variants verbatim:

```
SlotListNavigateUp, SlotListNavigateDown, SlotListSetOffset(usize, Modifiers),
SlotListScrollSeek(usize), SlotListActivateCenter, SlotListClickPlay(usize),
SlotListSelectionToggle(usize), SlotListSelectAllToggle, AddCenterToQueue,
SearchQueryChanged(String), SearchFocused(bool), SortModeSelected(SortMode),
ToggleSortOrder, RefreshViewData, CenterOnPlaying
```

Total: ~60 variants stamped across 8 files. A new slot-list feature requires 8 edits.

---

## 2. Design decisions (from research)

### Decision A: non-generic `SlotListPageMessage`

Use a non-generic `SlotListPageMessage` with `SortModeSelected(SortMode)`. Queue is the
only view that uses `QueueSortMode` — it will keep `SortModeSelected(QueueSortMode)` as a
per-view variant OUTSIDE the `SlotList(...)` wrapper. All other views share the same enum.

**Why not generic `<S>`**: Generic propagates to `SlotListPageState::handle()`, complicates
`SlotListPageAction`, and saves one enum variant in Queue while making the foundation harder.

### Decision B: expand `SlotListPageAction`

The current `SlotListPageAction { SearchChanged, SortModeChanged, SortOrderChanged, None }`
is too narrow. Phase A expands it to cover all navigation outcomes that require view-specific
interpretation.

### Decision C: non-recursive `ClickPlay`

`ClickPlay(idx)` in `handle()` calls `handle_set_offset(idx, total)` then returns
`SlotListPageAction::ActivateCenter`. The per-view arm treats them identically.

### Decision D: Radios has no special case

Research showed Radios does NOT do in-place sort. It delegates to SlotListPageState just like
library views. The plan doc note about in-place sort was stale. Radios gets the standard
treatment.

### Decision E: `impl_expansion_update!` macro is NOT touched

Albums, Artists, Genres, Playlists use `impl_expansion_update!` which handles
`SortModeSelected`, `ToggleSortOrder`, `SearchQueryChanged`, `SearchFocused` — and crucially,
the expansion sort/search methods call `self.expansion.clear()` before delegating to
`common.handle_sort_mode_selected()`. If these variants moved into `SlotList(...)`, that
`clear()` call would be lost (the unified `common.handle()` doesn't call it).

**Resolution**: For the 4 expansion views, `SlotList(...)` wraps only the pure-navigation
and activation variants. Sort/search (`SortModeSelected`, `ToggleSortOrder`,
`SearchQueryChanged`, `SearchFocused`) stay as per-view variants, still handled by the macro.
The macro source (`src/views/mod.rs`) is not modified. This keeps all parallel lanes
file-disjoint.

---

## 3. New types to add in `src/widgets/slot_list_page.rs`

### 3a. `SlotListPageMessage` enum

```rust
/// Shared slot-list message enum replacing stamped navigation variants.
/// Place directly above `SlotListPageAction` in slot_list_page.rs.
///
/// Expansion views (Albums/Artists/Genres/Playlists) emit all variants EXCEPT
/// SortModeSelected/ToggleSortOrder/SearchQueryChanged/SearchFocused — those
/// stay as per-view variants handled by impl_expansion_update! (which calls
/// self.expansion.clear() before delegating to common).
///
/// Non-expansion views (Songs/Queue/Radios/Similar) emit all variants. Queue
/// keeps SortModeSelected(QueueSortMode) per-view (different type) and does
/// not emit SlotList(SortModeSelected).
#[derive(Debug, Clone)]
pub enum SlotListPageMessage {
    // Navigation (all views)
    NavigateUp,
    NavigateDown,
    SetOffset(usize, iced::keyboard::Modifiers),
    ScrollSeek(usize),
    // Activation (all views except Similar which has no ActivateCenter/ClickPlay)
    ActivateCenter,
    ClickPlay(usize),
    // Selection (all views except Radios)
    SelectionToggle(usize),
    SelectAllToggle,
    // Queue/refresh/center (varies by view; views that don't need them just return None)
    AddCenterToQueue,
    RefreshViewData,
    CenterOnPlaying,
    // Sort/search (emitted by non-expansion views only; expansion views handle via macro)
    SearchQueryChanged(String),
    SearchFocused(bool),
    SortModeSelected(SortMode),     // Queue keeps SortModeSelected(QueueSortMode) per-view
    ToggleSortOrder,
}
```

### 3b. `SlotListPageAction` — expanded

```rust
#[derive(Debug, Clone)]
pub enum SlotListPageAction {
    None,
    ActivateCenter,     // view interprets: expand album, play song, play radio station, etc.
    AddCenterToQueue,   // view interprets: queue center item without playing
    RefreshViewData,    // view interprets: re-fetch from server / reload from AppService
    CenterOnPlaying,    // view interprets: scroll slot list to currently playing item
    SearchChanged(String),
    SortModeChanged(SortMode),
    SortOrderChanged(bool),
}
```

### 3c. `SlotListPageState::handle()` method

Add alongside the existing `handle_*` methods:

```rust
/// Unified dispatch for non-expansion views (Songs, Queue, Radios, Similar).
/// Expansion views (Albums, Artists, Genres, Playlists) do NOT call this method —
/// they match SlotList sub-variants individually using expansion-aware methods
/// (self.expansion.handle_navigate_up(items, &mut self.common), etc.).
/// `total` is the current item count (used for navigation bounds and search reset).
pub fn handle(&mut self, msg: SlotListPageMessage, total: usize) -> SlotListPageAction {
    match msg {
        SlotListPageMessage::NavigateUp => {
            self.handle_navigate_up(total);
            SlotListPageAction::None
        }
        SlotListPageMessage::NavigateDown => {
            self.handle_navigate_down(total);
            SlotListPageAction::None
        }
        SlotListPageMessage::SetOffset(offset, mods) => {
            self.handle_slot_click(offset, total, mods);
            SlotListPageAction::None
        }
        SlotListPageMessage::ScrollSeek(offset) => {
            self.handle_set_offset(offset, total);
            SlotListPageAction::None
        }
        SlotListPageMessage::SelectionToggle(offset) => {
            self.handle_selection_toggle(offset, total);
            SlotListPageAction::None
        }
        SlotListPageMessage::SelectAllToggle => {
            self.handle_select_all_toggle(total);
            SlotListPageAction::None
        }
        SlotListPageMessage::ActivateCenter => SlotListPageAction::ActivateCenter,
        SlotListPageMessage::ClickPlay(idx) => {
            self.handle_set_offset(idx, total);
            SlotListPageAction::ActivateCenter
        }
        SlotListPageMessage::AddCenterToQueue => SlotListPageAction::AddCenterToQueue,
        SlotListPageMessage::SearchQueryChanged(q) => {
            self.handle_search_query_changed(q, total)
        }
        SlotListPageMessage::SearchFocused(focused) => {
            self.handle_search_focused(focused);
            SlotListPageAction::None
        }
        SlotListPageMessage::SortModeSelected(mode) => self.handle_sort_mode_selected(mode),
        SlotListPageMessage::ToggleSortOrder => self.handle_toggle_sort_order(),
        SlotListPageMessage::RefreshViewData => SlotListPageAction::RefreshViewData,
        SlotListPageMessage::CenterOnPlaying => SlotListPageAction::CenterOnPlaying,
    }
}
```

**Note**: `handle_search_query_changed`, `handle_sort_mode_selected`, `handle_toggle_sort_order`
already return `SlotListPageAction` — pass them through directly.

### 3d. Export

Add to `src/widgets/mod.rs`:
```rust
pub(crate) use slot_list_page::SlotListPageMessage;
```

(Check if `SlotListPageAction` is already re-exported; if not, re-export it too.)

---

## 4. Per-view enum changes

Each per-view `*Message` enum gains one new variant and loses ~14:

| View | New variant | Variants removed | Kept per-view |
|------|------------|-----------------|---------------|
| Albums | `SlotList(SlotListPageMessage)` | **11** nav variants moved to SlotList; **4** sort/search kept per-view (macro) | SlotList covers: NavigateUp, NavigateDown, SetOffset, ScrollSeek, ActivateCenter, ClickPlay, SelectionToggle, SelectAllToggle, AddCenterToQueue, RefreshViewData, CenterOnPlaying. Kept per-view (macro): SortModeSelected, ToggleSortOrder, SearchQueryChanged, SearchFocused. View-specific: ClickSetRating, ClickToggleStar, ContextMenuAction, ExpandCenter, FocusAndExpand, CollapseExpansion, TracksLoaded, ArtworkLoaded, LargeArtworkLoaded, RefreshArtwork, NavigateAndFilter, NavigateAndExpandArtist, NavigateAndExpandGenre, ToggleColumnVisible, SetOpenMenu, ArtworkColumnDrag, Roulette |
| Artists | `SlotList(SlotListPageMessage)` | **11** nav variants moved to SlotList; **4** sort/search kept per-view (macro) | Same split as Albums. View-specific: ClickSetRating, ClickToggleStar, ContextMenuAction, ExpandCenter, FocusAndExpand, CollapseExpansion, AlbumsLoaded, NavigateAndExpandAlbum, NavigateAndFilter, OpenExternalUrl, SetOpenMenu, ArtworkColumnDrag, Roulette |
| Songs | `SlotList(SlotListPageMessage)` | **All** slot-list variants (14) moved to SlotList (no expansion) | SlotList covers all nav + sort/search. View-specific: ClickSetRating, ClickToggleStar, ContextMenuAction, RefreshArtwork, NavigateAndFilter, NavigateAndExpandAlbum, NavigateAndExpandArtist, NavigateAndExpandGenre, ToggleColumnVisible, SetOpenMenu, ArtworkColumnDrag, Roulette |
| Genres | `SlotList(SlotListPageMessage)` | **11** nav variants moved to SlotList; **4** sort/search kept per-view (macro) | Same split as Albums. View-specific: ClickToggleStar, ContextMenuAction, ExpandCenter, FocusAndExpand, CollapseExpansion, AlbumsLoaded, NavigateAndExpandAlbum, NavigateAndExpandArtist, NavigateAndFilter, SetOpenMenu, ArtworkColumnDrag, Roulette, ToggleColumnVisible |
| Playlists | `SlotList(SlotListPageMessage)` | **10** nav variants moved to SlotList (no CenterOnPlaying); **4** sort/search kept per-view (macro) | SlotList covers: NavigateUp, NavigateDown, SetOffset, ScrollSeek, ActivateCenter, ClickPlay, SelectionToggle, SelectAllToggle, AddCenterToQueue, RefreshViewData. Kept per-view (macro): SortModeSelected, ToggleSortOrder, SearchQueryChanged, SearchFocused. View-specific: ClickToggleStar, ContextMenuAction, PlaylistContextAction, ExpandCenter, FocusAndExpand, CollapseExpansion, TracksLoaded, NavigateAndFilter, NavigateAndExpandArtist, SetOpenMenu, ArtworkColumnDrag, Roulette, OpenDefaultPlaylistPicker, OpenCreatePlaylistDialog, ToggleColumnVisible |
| Queue | `SlotList(SlotListPageMessage)` | **10** variants (no SortModeSelected, no FocusCurrentPlaying, no RefreshViewData, no CenterOnPlaying) | SlotList covers: NavigateUp, NavigateDown, SetOffset, ScrollSeek, ActivateCenter, ClickPlay, SelectionToggle, SelectAllToggle, ToggleSortOrder, SearchQueryChanged. Kept per-view: SortModeSelected(QueueSortMode), FocusCurrentPlaying(usize, bool), ToggleColumnVisible(QueueColumn), and all Queue-specific variants |
| Radios | `SlotList(SlotListPageMessage)` | **All** slot-list variants (~13) moved to SlotList | SlotList covers all nav + sort/search + CenterOnPlaying + RefreshViewData. Kept per-view: FocusCurrentPlaying(String), EditStationDialog, DeleteStationConfirmation, CopyStreamUrl, AddRadioStation, NoOp, SetOpenMenu, RadioStationsLoaded |
| Similar | `SlotList(SlotListPageMessage)` | **7** nav/selection variants moved to SlotList (no sort/search/activation) | SlotList covers: NavigateUp, NavigateDown, SetOffset, ScrollSeek, SelectionToggle, SelectAllToggle, AddCenterToQueue. Kept per-view: NoOp, ClickToggleStar, ContextMenuAction, ToggleColumnVisible, SetOpenMenu, ArtworkColumnDrag |

**Queue note**: `QueueMessage::ToggleSortOrder` goes into `SlotList(SlotListPageMessage::ToggleSortOrder)` since it carries no type parameter. Only `SortModeSelected` stays per-view (different type: `QueueSortMode`).

**Similar note**: Similar's view() never emits ActivateCenter/ClickPlay (no activation semantics). The SlotList arm's match returns `SlotListPageAction::ActivateCenter` for those variants, which the Similar arm handles with `_ => {}` or an explicit `None` return.

---

## 5. Reference patterns

### Pattern A: Expansion view (Albums)

Albums uses `self.expansion.*` navigation methods that need the `albums` slice — the unified
`common.handle()` method is NOT used. The `SlotList(msg)` arm does an inner match and
moves existing arm bodies verbatim:

```rust
AlbumsMessage::SlotList(msg) => {
    use crate::widgets::SlotListPageMessage;
    match msg {
        SlotListPageMessage::NavigateUp => {
            let center = self.expansion.handle_navigate_up(albums, &mut self.common);
            match center {
                Some(idx) => (Task::none(), AlbumsAction::LoadLargeArtwork(idx.to_string())),
                None => (Task::none(), AlbumsAction::None),
            }
        }
        SlotListPageMessage::NavigateDown => {
            // same as NavigateUp, using handle_navigate_down
        }
        SlotListPageMessage::SetOffset(offset, modifiers) => {
            let center = self.expansion.handle_select_offset(offset, modifiers, albums, &mut self.common);
            match center {
                Some(idx) => (Task::none(), AlbumsAction::LoadLargeArtwork(idx.to_string())),
                None => (Task::none(), AlbumsAction::None),
            }
        }
        SlotListPageMessage::ScrollSeek(offset) => {
            self.expansion.handle_set_offset(offset, albums, &mut self.common);
            (Task::none(), AlbumsAction::None)
        }
        SlotListPageMessage::ClickPlay(offset) => {
            self.expansion.handle_set_offset(offset, albums, &mut self.common);
            self.update(AlbumsMessage::SlotList(SlotListPageMessage::ActivateCenter), total_items, albums)
        }
        SlotListPageMessage::SelectionToggle(offset) => {
            let flattened = self.expansion.flattened_len(albums);
            self.common.handle_selection_toggle(offset, flattened);
            (Task::none(), AlbumsAction::None)
        }
        SlotListPageMessage::SelectAllToggle => {
            let flattened = self.expansion.flattened_len(albums);
            self.common.handle_select_all_toggle(flattened);
            (Task::none(), AlbumsAction::None)
        }
        SlotListPageMessage::ActivateCenter => {
            // existing SlotListActivateCenter body moved here verbatim
        }
        SlotListPageMessage::AddCenterToQueue => {
            // existing AddCenterToQueue body moved here verbatim
        }
        SlotListPageMessage::RefreshViewData => (Task::none(), AlbumsAction::RefreshViewData),
        SlotListPageMessage::CenterOnPlaying => (Task::none(), AlbumsAction::CenterOnPlaying),
        // Sort/search handled by macro above; exhaustiveness arms only:
        SlotListPageMessage::SearchQueryChanged(_)
        | SlotListPageMessage::SearchFocused(_)
        | SlotListPageMessage::SortModeSelected(_)
        | SlotListPageMessage::ToggleSortOrder => (Task::none(), AlbumsAction::None),
    }
}
```

### Pattern B: Non-expansion view (Songs)

Songs calls `self.common.*` directly — `common.handle()` works cleanly:

```rust
SongsMessage::SlotList(msg) => {
    let total = songs.len();
    match self.songs_page.common.handle(msg, total) {
        SlotListPageAction::ActivateCenter => {
            // existing SlotListActivateCenter body
        }
        SlotListPageAction::AddCenterToQueue => {
            // existing AddCenterToQueue body
        }
        SlotListPageAction::SearchChanged(q) => (Task::none(), SongsAction::SearchChanged(q)),
        SlotListPageAction::SortModeChanged(m) => (Task::none(), SongsAction::SortModeChanged(m)),
        SlotListPageAction::SortOrderChanged(b) => (Task::none(), SongsAction::SortOrderChanged(b)),
        SlotListPageAction::RefreshViewData => (Task::none(), SongsAction::RefreshViewData),
        SlotListPageAction::CenterOnPlaying => (Task::none(), SongsAction::CenterOnPlaying),
        SlotListPageAction::None => (Task::none(), SongsAction::None),
    }
}
```

---

## 6. View file changes

Each view's `view.rs` (or equivalent) currently emits per-view slot-list variants directly
as widget callbacks and `on_*` parameters. After Phase A:

```rust
// Before:
RadiosMessage::SlotListNavigateUp,
RadiosMessage::SlotListNavigateDown,
move |f| RadiosMessage::SlotListScrollSeek((f * total as f32) as usize),
|station, ctx| { ... RadiosMessage::SlotListSetOffset(ctx.item_index, data.modifiers) ... }

// After:
use crate::widgets::SlotListPageMessage;
RadiosMessage::SlotList(SlotListPageMessage::NavigateUp),
RadiosMessage::SlotList(SlotListPageMessage::NavigateDown),
move |f| RadiosMessage::SlotList(SlotListPageMessage::ScrollSeek((f * total as f32) as usize)),
|station, ctx| { ... RadiosMessage::SlotList(SlotListPageMessage::SetOffset(ctx.item_index, data.modifiers)) ... }
```

The `on_roulette` field in `ViewHeaderConfig` stays unchanged — Roulette is not a
`SlotListPageMessage`.

---

## 7. `ViewPage` trait updates

`impl ViewPage for *Page` blocks implement `synth_set_offset_message()` which returns
`Message::Albums(AlbumsMessage::SlotListSetOffset(offset, Default::default()))`. After Phase A:

```rust
fn synth_set_offset_message(&self, offset: usize) -> Option<Message> {
    Some(Message::Albums(AlbumsMessage::SlotList(
        SlotListPageMessage::SetOffset(offset, iced::keyboard::Modifiers::default()),
    )))
}
```

Update each of the 6 implementing views (Albums, Artists, Songs, Genres, Playlists, Radios).
Queue and Similar don't implement this method.

---

## 8. Special cases

### Queue

Queue's update arm wraps the 11 shared variants in `SlotList(...)` and keeps:
```rust
// These stay as separate per-view arms (not inside SlotList):
QueueMessage::SortModeSelected(mode) => { ... }   // carries QueueSortMode
QueueMessage::FocusCurrentPlaying(idx, start) => { ... }
QueueMessage::ToggleColumnVisible(col) => { ... }
```

Queue has no `handle_seek_settled` scroll-seek fast-path since it doesn't use
`dispatch_view_with_seek!`. Verify in root `mod.rs` that Queue's arm is plain
`Message::Queue(msg) => self.handle_queue(msg)` — no change needed there.

### Similar

Similar's `view()` only emits navigation/selection messages. Its update arm:
```rust
SimilarMessage::SlotList(msg) => {
    let total = self.similar_songs.len();
    match self.similar_page.common.handle(msg, total) {
        SlotListPageAction::None => {}
        // All other variants → None (Similar has no sort/search/center/queue ops via SlotList)
        _ => {}
    }
    (Task::none(), SimilarAction::None)
}
```

The view still emits `SimilarMessage::ClickToggleStar(usize)` and `SimilarMessage::AddCenterToQueue`
separately if those exist — double-check the Similar view to see if AddCenterToQueue is
emitted directly or via SlotList.

---

## 9. Execution plan

### Lane 0 (foundation — must land first, blocks all other lanes)

**Agent task**: Single worktree agent.

`impl_expansion_update!` macro in `src/views/mod.rs` is **NOT modified**.

Files changed:
- `src/widgets/slot_list_page.rs` — add `SlotListPageMessage` enum (above `SlotListPageAction`),
  expand `SlotListPageAction` with new variants, add `SlotListPageState::handle()` method
- `src/widgets/mod.rs` — add `pub(crate) use slot_list_page::SlotListPageMessage;`
- `src/views/albums/mod.rs` — add `SlotList(SlotListPageMessage)` variant, remove 11 nav variants
  (keep SortModeSelected/ToggleSortOrder/SearchQueryChanged/SearchFocused as per-view, macro unchanged)
- `src/views/albums/update.rs` — add one `AlbumsMessage::SlotList(msg)` arm in the `Err(msg)` block
  (the 11 nav arms it replaces are all below the macro invocation in the `Err(msg)` block)
- `src/views/albums/view.rs` — update widget callbacks: `AlbumsMessage::SlotListNavigateUp` →
  `AlbumsMessage::SlotList(SlotListPageMessage::NavigateUp)` etc. (11 sites)
- `src/views/albums/mod.rs` impl ViewPage — update `synth_set_offset_message`

CI gate before merging: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings`

**Commit message**: `refactor(views): add SlotListPageMessage carrier + migrate Albums (Phase A foundation)`

---

### Lanes 1–7 (parallel — all start from foundation-merged main)

All lanes are file-disjoint. Launch all 7 simultaneously after foundation merges.

| Lane | View | SlotList covers | Files touched |
|------|------|----------------|--------------|
| 1 | Artists | 11 nav variants (sort/search kept per-view via macro) | `views/artists/mod.rs`, `views/artists/update.rs`, `views/artists/view.rs` |
| 2 | Songs | All 14 slot-list variants (no expansion) | `views/songs/mod.rs`, `views/songs/update.rs`, `views/songs/view.rs` |
| 3 | Genres | 11 nav variants (sort/search kept per-view via macro) | `views/genres/mod.rs`, `views/genres/update.rs`, `views/genres/view.rs` |
| 4 | Playlists | 10 nav variants (no CenterOnPlaying; sort/search kept per-view via macro) | `views/playlists/mod.rs`, `views/playlists/update.rs`, `views/playlists/view.rs` |
| 5 | Queue | 10 variants (keep SortModeSelected/FocusCurrentPlaying/ToggleColumnVisible per-view) | `views/queue/mod.rs`, `views/queue/update.rs`, `views/queue/view.rs` |
| 6 | Radios | All ~13 slot-list variants (keep FocusCurrentPlaying per-view) | `views/radios.rs` |
| 7 | Similar | 7 nav/selection variants (no sort/search/activation) | `views/similar.rs` |

Each lane also updates its `impl ViewPage` block's `synth_set_offset_message` where applicable.

Each lane CI gate: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo +nightly fmt --all -- --check`

---

## 10. Test migration notes

**Navigation macro tests** (`src/update/tests/navigation.rs`): **No changes needed.** These
tests invoke high-level handler methods and assert on state mutations and `*Action` variants,
not on `*Message` variants. They're resilient to this refactor.

**View-specific tests** in `src/update/tests/*.rs` and inline `#[cfg(test)]` blocks: Grep
each view's test files for `SlotListNavigateUp`, `SlotListNavigateDown`, `SlotListSetOffset`,
etc. before starting each lane. If found, update to
`SlotList(SlotListPageMessage::NavigateUp)` etc. The foundation agent should do this grep
for Albums; parallel agents do it for their view.

Quick grep to run per lane:
```bash
grep -rn "SlotListNavigate\|SlotListSetOffset\|SlotListScrollSeek\|SlotListActivate\|SlotListClickPlay\|SlotListSelection" \
    src/update/tests/ src/views/<viewname>/
```

---

## 11. Audit tracker update

After all lanes merge:
- §4 #9 → ✅ done (all three phases complete: C ✅, B superseded ✅, A ✅)
- Append commit refs to the §4 #9 row in `.agent/audit-progress.md`

---

## 12. What this plan does NOT include

- Removing the `HasViewChrome` trait or `dispatch_view_chrome` — those stay
- Changing `SlotListPageState`'s internal data structures
- Migrating the `impl_expansion_update!` macro — it handles expansion messages, not slot-list nav
- The queued plan batches (`batch1-small-fixes`, `batch2-handler-dry`, etc.) — separate items
