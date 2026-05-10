# Loaders + Chrome — §3 #9 + §3 #10 + §3 #11 (fanout plan)

Closes three open audit items: paginated library loader consolidation, test fixture helpers,
and handler prologue extraction into a `dispatch_view_chrome` free function.

All three lanes are runnable in parallel. Lanes B and C share six handler files but touch
**different function bodies** — prologues in B, loader functions in C. Merge order: A, B, C
(or A and B together, then C). Merge conflicts are not expected if each lane stays in scope.

Last verified baseline: **2026-05-10, `main @ HEAD`**.

Research sources (synthesized 2026-05-10):
- `~/nokkvi-audit-results/` §3 #9, §3 #10, §3 #11 sections
- Live codebase grep + read pass confirming line numbers

---

## 1. Goal & rubric

| Lane | Item | Files touched | Effort |
|---|---|---|---|
| **A** (test helpers) | §3 #10 — `expand_*_with()` + `make_settings_view_data` promotion | `src/update/tests/test_helpers.rs`, `general.rs`, `playback.rs`, `navigation.rs`, `queue.rs`, `tests_star_rating.rs` | XS |
| **B** (view chrome) | §3 #11 — `HasViewChrome` trait + `dispatch_view_chrome` free fn | NEW `src/update/chrome.rs`, `albums.rs`, `artists.rs`, `songs.rs`, `genres.rs`, `playlists.rs`, `queue.rs`, `radios.rs`, `similar.rs` | M |
| **C** (paged loaders) | §3 #9 — `paginated_load_task` pattern; fix Radios `set_loading` bug | `albums.rs`, `artists.rs`, `songs.rs`, `genres.rs`, `playlists.rs`, `radios.rs` | M |

---

## 2. Conflict zones

Lanes B and C both touch six handler files — but at disjoint function bodies.

| File | Lane B touches | Lane C touches |
|---|---|---|
| `src/update/albums.rs` | `fn handle_albums` prologue (first ~20 lines) | `fn load_albums_internal` (L27–91), `fn handle_load_albums` (L93–106), `fn handle_albums_load_page` (L109–116), `fn force_load_albums_page` (L122–129) |
| `src/update/artists.rs` | `fn handle_artists` prologue | `fn load_artists_internal` (L37–106), wrappers (L108–142) |
| `src/update/songs.rs` | `fn handle_songs` prologue | `fn load_songs_internal` (L21–78), wrappers (L80–116) |
| `src/update/genres.rs` | `fn handle_genres` prologue | `fn handle_load_genres` (L15–60) — full function |
| `src/update/playlists.rs` | `fn handle_playlists` prologue | `fn handle_load_playlists` (L15–63) — full function |
| `src/update/radios.rs` | `fn handle_radios` prologue | `fn handle_load_radio_stations` (L13–36) — full function |
| `src/update/similar.rs` | `fn handle_similar_message` prologue (SetOpenMenu only) | — not touched |

Lane A touches only test files — no overlap with B or C.

Lane B creates `src/update/chrome.rs` (new file). Lane C may create `src/update/loaders.rs` or inline the trait. No file conflict.

**Merge strategy**: run all three in parallel worktrees. After all complete: merge A (clean), merge B (clean), merge C. If B+C do conflict on a handler file, the conflict will be minimal (different line ranges) and auto-resolvable.

---

## 3. Per-lane scope

### Lane A — §3 #10: Test fixture helpers

**Status**: Already ~80% done. The bulk fixtures (`albums_indexed`, `seed_*`, `arm_pending_*`, macro infrastructure) landed in prior commits. Two things remain:

#### A.1 — Add `expand_*_with()` helpers (21 call sites)

Add to `src/update/tests/test_helpers.rs`:

```rust
pub(crate) fn expand_albums_with(app: &mut Nokkvi, id: &str, children: Vec<SongUIViewData>) {
    app.albums_page.expansion.expanded_id = Some(id.into());
    app.albums_page.expansion.parent_offset = 0;
    app.albums_page.expansion.children = children;
}
pub(crate) fn expand_artists_with(app: &mut Nokkvi, id: &str, children: Vec<AlbumUIViewData>) {
    app.artists_page.expansion.expanded_id = Some(id.into());
    app.artists_page.expansion.parent_offset = 0;
    app.artists_page.expansion.children = children;
}
pub(crate) fn expand_genres_with(app: &mut Nokkvi, id: &str, children: Vec<AlbumUIViewData>) {
    app.genres_page.expansion.expanded_id = Some(id.into());
    app.genres_page.expansion.parent_offset = 0;
    app.genres_page.expansion.children = children;
}
pub(crate) fn expand_playlists_with(app: &mut Nokkvi, id: &str, children: Vec<SongUIViewData>) {
    app.playlists_page.expansion.expanded_id = Some(id.into());
    app.playlists_page.expansion.parent_offset = 0;
    app.playlists_page.expansion.children = children;
}
```

Call sites: `navigation.rs` (12), `queue.rs` (5), `tests_star_rating.rs` (4) — all pattern `.expansion.expanded_id = Some(...)` + `.expansion.parent_offset = 0` + `.expansion.children = ...`.

#### A.2 — Promote `make_settings_view_data()`

Move the 60-line `fn make_settings_view_data()` from `src/update/tests/general.rs:26–85` to `test_helpers.rs`. Change visibility from `pub(super)` to `pub(crate)`. Fix the import in `playback.rs` (currently `use super::general::make_settings_view_data`).

**Commit message**:

    test(helpers): expand_*_with helpers + promote make_settings_view_data (§3 #10)

    Add expand_{albums,artists,genres,playlists}_with() to test_helpers.rs.
    Replace 21 inline expansion setup blocks (navigation×12, queue×5,
    tests_star_rating×4) with helper calls.

    Promote make_settings_view_data from pub(super) in general.rs to
    pub(crate) in test_helpers.rs. Fix the cross-file import in playback.rs.

    Closes audit §3 #10 (.agent/audit-progress.md §3).

---

### Lane B — §3 #11: `dispatch_view_chrome` + `HasViewChrome` trait

**The problem**: Every `handle_*` function (Albums, Artists, Songs, Genres, Playlists, Queue, Radios, Similar) begins with 1–3 identical blocks:

1. `SetOpenMenu` early-return (all 8)
2. `Roulette` early-return (7 of 8; Similar omits)
3. `play_view_sfx(nav_flag, expand_flag)` call (7 of 8; Similar omits)

**The fix**: Create `src/update/chrome.rs` with a `HasViewChrome` trait and `dispatch_view_chrome<M: HasViewChrome>` free function. Each handler file adds a trait impl; the prologue collapses to one call.

#### B.1 — `chrome.rs` design

```rust
// src/update/chrome.rs
pub(crate) trait HasViewChrome {
    fn extract_set_open_menu(&self) -> Option<Option<crate::app_message::OpenMenu>>;
    fn is_roulette(&self) -> bool;
    fn is_nav_action(&self) -> bool;
    fn is_expand_action(&self) -> bool;
}

/// Returns Some(task) if the message was a chrome intercept (caller should return it).
/// Returns None if the message should proceed to the page's update().
pub(crate) fn dispatch_view_chrome<M: HasViewChrome>(
    handler: &mut crate::Nokkvi,
    msg: &M,
    view: crate::View,
) -> Option<iced::Task<crate::app_message::Message>> {
    use crate::app_message::Message;

    if let Some(menu) = msg.extract_set_open_menu() {
        return Some(iced::Task::done(Message::SetOpenMenu(menu)));
    }
    if msg.is_roulette() {
        return Some(iced::Task::done(Message::Roulette(
            crate::app_message::RouletteMessage::Start(view),
        )));
    }
    handler.play_view_sfx(msg.is_nav_action(), msg.is_expand_action());
    None
}
```

Re-export from `src/update/mod.rs`:
```rust
pub(crate) use chrome::dispatch_view_chrome;
```

#### B.2 — Per-view trait impls and which nav/expand variants to match

| View | `is_expand_action` variants | Notes |
|---|---|---|
| Albums | `CollapseExpansion \| ExpandCenter` | — |
| Artists | `CollapseExpansion \| ExpandCenter` | Also has `OpenExternalUrl` branch between Roulette and play_view_sfx — preserved in handler |
| Genres | `CollapseExpansion \| ExpandCenter` | — |
| Playlists | `CollapseExpansion \| ExpandCenter` | — |
| Songs | always `false` | Songs has no expand/collapse |
| Queue | always `false` | Queue has no expand; also has SlotListScrollSeek fast-path before SFX |
| Radios | always `false` | — |
| Similar | no trait needed (no Roulette, no SFX) | Only SetOpenMenu is inlined; leave as-is or impl with all false |

All views: `is_nav_action` matches `SlotListNavigateUp \| SlotListNavigateDown`.

#### B.3 — Queue special case

Queue has a fast-path for `SlotListScrollSeek` that must execute BEFORE `play_view_sfx`. The `dispatch_view_chrome` call should be placed BEFORE the fast-path, since it only handles SetOpenMenu/Roulette (which are early-returns). The play_view_sfx call inside `dispatch_view_chrome` runs AFTER both early-return checks — so if SlotListScrollSeek reaches it, it's fine because `is_nav_action()` will return false for ScrollSeek. The current fast-path at lines 121–145 just skips SFX for that variant, which `dispatch_view_chrome` also achieves by returning `None` and letting the handler fall through.

#### B.4 — Handler after refactor (example)

```rust
pub(crate) fn handle_albums(&mut self, msg: views::AlbumsMessage) -> Task<Message> {
    if let Some(task) = dispatch_view_chrome(self, &msg, crate::View::Albums) {
        return task;
    }
    // ... rest of handler unchanged ...
}
```

**Commit message**:

    refactor(update): dispatch_view_chrome + HasViewChrome extract handler prologues (§3 #11)

    Every handle_*() function began with 2–3 identical blocks: SetOpenMenu
    early-return, Roulette early-return, play_view_sfx nav/expand call.
    7 full sites + 1 partial (similar — SetOpenMenu only).

    Add chrome.rs with HasViewChrome trait and dispatch_view_chrome<M> free fn.
    Impl the trait for 7 message types. Collapse each handler prologue to a
    single dispatch_view_chrome() call.

    Artists preserves its OpenExternalUrl branch; Queue preserves its
    SlotListScrollSeek fast-path. Similar keeps inline SetOpenMenu only
    (no Roulette, no SFX).

    Closes audit §3 #11 (.agent/audit-progress.md §3).

---

### Lane C — §3 #9: Paginated loader consolidation

**The problem**: Six views (Albums, Artists, Songs, Genres, Playlists, Radios) each implement
a library-load function with the same structure: `set_loading(true)`, `shell_task(fetch, transform)`.
Three of them (Albums/Artists/Songs) factor this into `load_*_internal`, but all six duplicate
the pattern. Two bugs: Radios omits `set_loading(true)` entirely; non-paged views use
`e.to_string()` instead of `format!("{e:#}")`.

**File-by-file scope** (ONLY these functions; no other changes):

| File | Functions to consolidate |
|---|---|
| `albums.rs:27–129` | `load_albums_internal`, `handle_load_albums`, `handle_albums_load_page`, `force_load_albums_page` |
| `artists.rs:37–142` | `load_artists_internal`, `handle_load_artists`, `handle_artists_load_page`, `force_load_artists_page` |
| `songs.rs:21–116` | `load_songs_internal`, `handle_load_songs`, `handle_songs_load_page`, `force_load_songs_page` |
| `genres.rs:15–60` | `handle_load_genres` |
| `playlists.rs:15–63` | `handle_load_playlists` |
| `radios.rs:13–36` | `handle_load_radio_stations` |

**Implementation approach** — choose based on what compiles cleanly:

**Option A (macro)**: `macro_rules! paginated_load!` that expands inline in each function. Avoids all borrow-checker issues since macros expand at the call site and `self` is available.

**Option B (free function + borrow split)**: Pass the `PagedBuffer` ref separately from `self.shell_task`. Concretely:
```rust
pub(crate) fn paginated_load_task<T, U>(
    buffer: &mut PagedBuffer<U>,
    shell_task_fn: impl FnOnce(...) -> Task<Message>,
    fetch_fn: ...,
    transform_fn: ...,
) -> Task<Message>
```
This avoids the dual `&mut self` issue by receiving the pre-mutated buffer ref and a pre-bound `shell_task_fn`.

**What MUST be standardized** regardless of approach:
1. `set_loading(true)` on the buffer before every fetch (fix: Radios currently omits this)
2. Error formatting: `format!("{e:#}")` everywhere (currently genres/playlists/radios use `e.to_string()`)
3. The 3 wrapper fns (`handle_load_*`, `handle_*_load_page`, `force_load_*_page`) for paged views — if the helper makes `load_*_internal` small enough, consider inlining the wrappers

**Commit message**:

    refactor(update): consolidate paginated library loader pattern (§3 #9)

    Albums, artists, songs each had a 58–70 line load_*_internal + 3 wrapper
    fns. Genres, playlists, radios inlined the pattern directly. All six shared
    the same set_loading(true) + shell_task structure with per-domain variation.

    [describe chosen approach: macro / free fn / other]

    Bug fixed: radios.rs was missing set_loading(true) before the fetch,
    risking duplicate load races on rapid navigation. Error formatting
    normalized to format!("{e:#}") across all six loaders.

    Closes audit §3 #9 (.agent/audit-progress.md §3).

---

## 4. Signature-interlock matrix

| Signal | Lane A | Lane B | Lane C |
|---|---|---|---|
| `expand_*_with()` fns | **adds** | — | — |
| `make_settings_view_data()` | **moves** to test_helpers | — | — |
| `HasViewChrome` trait | — | **adds** in chrome.rs | — |
| `dispatch_view_chrome()` fn | — | **adds** in chrome.rs | — |
| `handle_albums` prologue | — | **replaces** 3 blocks | not touched |
| `load_albums_internal` | — | not touched | **replaces or extracts** |
| `handle_load_albums` | — | not touched | **replaces or extracts** |
| `handle_load_genres` full fn | — | not touched | **replaces or extracts** |
| `radio_stations.set_loading(true)` | — | — | **adds** (bug fix) |

No lane modifies a symbol another lane is adding or removing. ✓

---

## 5. Verification (all lanes)

Per `.agent/workflows/build-test.md`:

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass. No `#[allow]` attributes. Lane A: `cargo test` is the primary signal (test helper changes should make tests shorter, not break them).

---

## 6. What each lane does NOT do

- **Lane A**: Does NOT add new test scenarios. Only adds helpers and migrates existing setups to them. Does NOT touch any production code files.
- **Lane B**: Does NOT change the logic of SetOpenMenu/Roulette/play_view_sfx. Only extracts the same code into a trait + free fn. Does NOT touch the page's `update()` logic after the prologue. Does NOT change Similar's behavior (it had no Roulette/SFX; it still won't).
- **Lane C**: Does NOT touch `handle_albums` or any of the other main handler dispatch functions. Only touches the loader functions. Does NOT change message types or result handling. Does NOT add `set_loading(false)` — that's already handled in the downstream `handle_*_loaded` handlers.

---

## 7. After all lanes land

Update `.agent/audit-progress.md`:
- §3 row 9 → ✅ done
- §3 row 10 → ✅ done
- §3 row 11 → ✅ done

---

## Fanout Prompts

### lane-a-test-helpers

worktree: ~/nokkvi-loaders-chrome-a
branch: refactor/test-helpers-expand-fns
effort: max
permission-mode: bypassPermissions

````
Task: §3 #10 — add expand_*_with() test helpers and promote make_settings_view_data() to test_helpers.rs.

Plan doc: /home/foogs/nokkvi/.agent/plans/loaders-and-chrome.md (section 3 "Lane A").

Working directory: ~/nokkvi-loaders-chrome-a (worktree pre-created). Branch: refactor/test-helpers-expand-fns. Do NOT run `git worktree add`.

## Context

The nokkvi test suite already has `seed_*`, `albums_indexed`, `arm_pending_*` helpers in
`src/update/tests/test_helpers.rs`. Two patterns remain duplicated across 21 test functions:

1. **Inline expansion setup**: `.expansion.expanded_id = Some("x".to_string()); .expansion.parent_offset = 0; .expansion.children = vec![...]` — appears 21 times across navigation.rs (12), queue.rs (5), tests_star_rating.rs (4).

2. **make_settings_view_data**: 60-line function defined `pub(super)` in `general.rs` (lines 26–85). Imported via `use super::general::make_settings_view_data` in `playback.rs`. Should be `pub(crate)` in `test_helpers.rs`.

## What to do

### 1. Read the existing helpers

Read `src/update/tests/test_helpers.rs` fully to understand the existing helper pattern
(especially `seed_albums`, `make_album`) and find a good insertion point.

### 2. Read the target test files to understand what types are used

- Read `src/update/tests/navigation.rs` — look for `.expansion.expanded_id` to see the duplication.
- Read `src/update/tests/queue.rs` — same.
- Read `src/update/tests/tests_star_rating.rs` — same.
- Note what child type each view uses (albums expansion has `SongUIViewData` children, artists/genres have `AlbumUIViewData` children, playlists have `SongUIViewData` children).

### 3. Add expand_*_with() helpers to test_helpers.rs

Add four helpers after the existing `arm_pending_*` helpers:

```rust
pub(crate) fn expand_albums_with(app: &mut Nokkvi, id: &str, children: Vec<SongUIViewData>) {
    app.albums_page.expansion.expanded_id = Some(id.into());
    app.albums_page.expansion.parent_offset = 0;
    app.albums_page.expansion.children = children;
}
// ... artists (Vec<AlbumUIViewData>), genres (Vec<AlbumUIViewData>), playlists (Vec<SongUIViewData>)
```

Verify the actual field types by reading the expansion struct in the source before writing.

### 4. Update the 21 call sites

In each of navigation.rs (12 sites), queue.rs (5 sites), tests_star_rating.rs (4 sites):
Replace the 3-line inline setup with a call to the appropriate `expand_*_with(...)` helper.
Import via `use super::test_helpers::expand_albums_with;` etc. (or use the existing wildcard import if present).

### 5. Promote make_settings_view_data

In `general.rs`: cut the function body (it will become a delegation or just be moved).
In `test_helpers.rs`: paste it with `pub(crate)` visibility.
In `playback.rs`: update the import to `use super::test_helpers::make_settings_view_data;` (or equivalent based on module structure).

### 6. Verify

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

### 7. Commit

    test(helpers): expand_*_with helpers + promote make_settings_view_data (§3 #10)

    Add expand_{albums,artists,genres,playlists}_with() to test_helpers.rs.
    Replace 21 inline expansion setup blocks (navigation×12, queue×5,
    tests_star_rating×4) with helper calls.

    Promote make_settings_view_data from pub(super) in general.rs to
    pub(crate) in test_helpers.rs. Fix the cross-file import in playback.rs.

    Closes audit §3 #10 (.agent/audit-progress.md §3).

Skip the Co-Authored-By trailer.

### 8. Update audit tracker

Append commit ref to `.agent/audit-progress.md` §3 row 10, flip to ✅ done.

## What NOT to touch

- Any production code files (only test files).
- The macro infrastructure in navigation_macros.rs — already complete.
- Other test helpers that already exist.

## If blocked

- If `expansion.parent_offset` doesn't exist on the expansion struct (field name may differ):
  read the struct definition in `src/views/*/mod.rs` or wherever `*PageState` is defined.
- If the child type guesses are wrong: look at the existing inline code to see what's being assigned.

## Reporting

End with: commit ref, count of call sites migrated, confirmation that `cargo test` passes.
````

---

### lane-b-view-chrome

worktree: ~/nokkvi-loaders-chrome-b
branch: refactor/view-chrome
effort: max
permission-mode: bypassPermissions

````
Task: §3 #11 — extract the SetOpenMenu/Roulette/play_view_sfx handler prologues into a HasViewChrome trait + dispatch_view_chrome() free function.

Plan doc: /home/foogs/nokkvi/.agent/plans/loaders-and-chrome.md (section 3 "Lane B").

Working directory: ~/nokkvi-loaders-chrome-b (worktree pre-created). Branch: refactor/view-chrome. Do NOT run `git worktree add`.

## Context

Every `handle_*()` function in `src/update/` begins with 2–3 identical blocks:

1. `if let XxxxMessage::SetOpenMenu(next) = msg { return Task::done(Message::SetOpenMenu(next)); }`
2. `if matches!(msg, XxxxMessage::Roulette) { return Task::done(Message::Roulette(...)); }`
3. `self.play_view_sfx(matches!(msg, Nav variants), matches!(msg, Expand variants));`

These appear in: albums.rs (~L374), artists.rs (~L225), songs.rs (~L148), genres.rs (~L127),
playlists.rs (~L73), queue.rs (~L112), radios.rs (~L59). Similar.rs (~L19) has SetOpenMenu only.

## What to do

### 1. Read the existing prologues

Read the first 40 lines of each handle_* function in:
- `src/update/albums.rs` — full prologue (SetOpenMenu + Roulette + play_view_sfx with expand=true)
- `src/update/songs.rs` — nav-only prologue (expand=false)
- `src/update/artists.rs` — full prologue + OpenExternalUrl branch (view-specific, preserve it)
- `src/update/queue.rs` — nav-only + SlotListScrollSeek fast-path
- `src/update/similar.rs` — SetOpenMenu only, no Roulette, no SFX
- `src/update/genres.rs`, `playlists.rs`, `radios.rs` — full prologues

Also read `src/update/mod.rs` to understand how to `pub(crate) use` from a new module.

### 2. Create src/update/chrome.rs

```rust
use iced::Task;
use crate::app_message::{Message, OpenMenu};

pub(crate) trait HasViewChrome {
    fn extract_set_open_menu(&self) -> Option<Option<OpenMenu>>;
    fn is_roulette(&self) -> bool;
    fn is_nav_action(&self) -> bool;
    fn is_expand_action(&self) -> bool;
}

/// Returns Some(task) if msg was a chrome intercept (return it immediately).
/// Returns None if msg should continue to the page's update().
pub(crate) fn dispatch_view_chrome<M: HasViewChrome>(
    handler: &mut crate::Nokkvi,
    msg: &M,
    view: crate::View,
) -> Option<Task<Message>> {
    if let Some(menu) = msg.extract_set_open_menu() {
        return Some(Task::done(Message::SetOpenMenu(menu)));
    }
    if msg.is_roulette() {
        return Some(Task::done(Message::Roulette(
            crate::app_message::RouletteMessage::Start(view),
        )));
    }
    handler.play_view_sfx(msg.is_nav_action(), msg.is_expand_action());
    None
}
```

Add `pub(crate) mod chrome;` and `pub(crate) use chrome::dispatch_view_chrome;` to `src/update/mod.rs`.

### 3. Implement HasViewChrome for each message type

For each of Albums, Artists, Songs, Genres, Playlists, Queue, Radios — add an impl block.
The nav variants are `SlotListNavigateUp | SlotListNavigateDown` for all views.
The expand variants differ:
- Albums, Artists, Genres, Playlists: `CollapseExpansion | ExpandCenter` (verify exact names from the existing prologue)
- Songs, Queue, Radios: `false` (no expand/collapse)

Read the existing prologue match patterns for the exact variant names before writing impls.

For `extract_set_open_menu`: the SetOpenMenu variant carries `Option<OpenMenu>`. Return `Some(inner)`.

For Similar: if it has no Roulette and no SFX, you can either skip implementing the trait
or impl it with `is_roulette() -> false`, `is_expand_action() -> false` for uniformity.
Leave the SetOpenMenu in similar.rs as-is (it's only 4 lines).

### 4. Replace prologues in handler files

In each of albums.rs, artists.rs, songs.rs, genres.rs, playlists.rs, queue.rs, radios.rs:
Replace the 3-block prologue with:
```rust
if let Some(task) = dispatch_view_chrome(self, &msg, crate::View::Albums) {
    return task;
}
```
(adjust View variant per file)

**Artists special case**: After the `dispatch_view_chrome` call (which handles SetOpenMenu and Roulette),
preserve the `OpenExternalUrl` branch that currently sits between Roulette and play_view_sfx.
Move it to just after the `dispatch_view_chrome` call.

**Queue special case**: The `SlotListScrollSeek` fast-path currently precedes play_view_sfx.
After the refactor, the `dispatch_view_chrome` call handles SetOpenMenu and Roulette (early returns),
and internally calls `play_view_sfx` for other messages. For ScrollSeek, `is_nav_action()` returns
false and `is_expand_action()` returns false, so play_view_sfx(false, false) is a no-op — correct.
The fast-path should move to AFTER the `dispatch_view_chrome` call.

### 5. Verify

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

### 6. Commit

    refactor(update): dispatch_view_chrome + HasViewChrome extract handler prologues (§3 #11)

    Every handle_*() function began with 2–3 identical blocks: SetOpenMenu
    early-return, Roulette early-return, play_view_sfx nav/expand call.
    7 full sites + 1 partial (similar — SetOpenMenu only).

    Add chrome.rs with HasViewChrome trait and dispatch_view_chrome<M> free fn.
    Impl the trait for 7 message types. Collapse each handler prologue to a
    single dispatch_view_chrome() call.

    Artists preserves its OpenExternalUrl branch; Queue's SlotListScrollSeek
    fast-path is preserved after the chrome dispatch.

    Closes audit §3 #11 (.agent/audit-progress.md §3).

Skip the Co-Authored-By trailer.

### 7. Update audit tracker

Append commit ref to `.agent/audit-progress.md` §3 row 11, flip to ✅ done.

## What NOT to touch

- `handle_albums` logic after the prologue — only the 3-block prologue at the top.
- The page's `update()` function — this refactor only changes the dispatcher prologue.
- similar.rs SetOpenMenu handling (it has no Roulette/SFX; keep it simple).
- `load_*_internal`, `handle_load_*`, `force_load_*_page` functions — those are Lane C's scope.

## If blocked

- If `OpenMenu` type path differs from `crate::app_message::OpenMenu`: grep for its definition.
- If `play_view_sfx` is private and can't be called from chrome.rs: make it `pub(crate)` first.
- If the trait bound conflicts with `Clone` on Message: add the constraint to dispatch_view_chrome's where clause.

## Reporting

End with: commit ref, which files changed, how many prologue blocks removed, whether `cargo test` passes.
````

---

### lane-c-paged-loaders

worktree: ~/nokkvi-loaders-chrome-c
branch: refactor/paginated-loaders
effort: max
permission-mode: bypassPermissions

````
Task: §3 #9 — consolidate the paginated library loader pattern across Albums, Artists, Songs, Genres, Playlists, Radios. Fix Radios missing set_loading(true). Normalize error formatting.

Plan doc: /home/foogs/nokkvi/.agent/plans/loaders-and-chrome.md (section 3 "Lane C").

Working directory: ~/nokkvi-loaders-chrome-c (worktree pre-created). Branch: refactor/paginated-loaders. Do NOT run `git worktree add`.

## Context

Six views implement library loading with the same skeleton:
  1. `set_loading(true)` on the buffer
  2. `shell_task(fetch_closure, msg_ctor)`

The paged views (Albums, Artists, Songs) factor this into `load_*_internal(offset, force, msg_ctor)` + 3 wrappers. The non-paged views (Genres, Playlists, Radios) inline it directly. Radios has a bug: it omits `set_loading(true)`. Non-paged views use `e.to_string()` for errors; paged use `format!("{e:#}")` — inconsistency bites in bug reports.

IMPORTANT: You are ONLY touching the loader functions listed below. Do NOT touch `handle_albums`, `handle_genres`, etc. (the main dispatch handlers) — those prologues are Lane B's scope.

## Files and functions in scope

- `src/update/albums.rs:27–129`: `load_albums_internal`, `handle_load_albums`, `handle_albums_load_page`, `force_load_albums_page`
- `src/update/artists.rs:37–142`: `load_artists_internal`, `handle_load_artists`, `handle_artists_load_page`, `force_load_artists_page`
- `src/update/songs.rs:21–116`: `load_songs_internal`, `handle_load_songs`, `handle_songs_load_page`, `force_load_songs_page`
- `src/update/genres.rs:15–60`: `handle_load_genres` (full function)
- `src/update/playlists.rs:15–63`: `handle_load_playlists` (full function)
- `src/update/radios.rs:13–36`: `handle_load_radio_stations` (full function)

## What to do

### 1. Read all six loader sites

Read each of the files and functions listed above. For each, note:
- The AppService method called (e.g., `albums_vm.load_raw_albums_page`, `shell.genres_api().await`)
- The transform applied to items
- The message constructor
- Whether `set_loading(true)` is called
- The error arm: `e.to_string()` or `format!("{e:#}")`

Also read `src/update/components.rs` around `PaginatedFetch` to understand the struct.

### 2. Choose an implementation approach

**Preferred — macro approach** (avoids borrow checker split):

Define a `macro_rules! paginated_load!` in a new file `src/update/loaders.rs`:
```rust
macro_rules! paginated_load {
    (
        app: $app:expr,
        buffer: $buf:expr,
        fetch: $fetch:expr,
        transform: $transform:expr,
        msg: $msg:expr $(,)?
    ) => {{
        $buf.set_loading(true);
        $app.shell_task(
            move |shell| async move {
                match ($fetch)(shell).await {
                    Ok((items, total)) => (Ok(items.into_iter().map($transform).collect()), total),
                    Err(e) => (Err(format!("{e:#}")), 0),
                }
            },
            $msg,
        )
    }};
}
pub(crate) use paginated_load;
```

Use as: `paginated_load!(app: self, buffer: &mut self.library.albums, fetch: |shell| async move {...}, transform: |a| AlbumUIViewData::from_album(a, &url, &cred), msg: msg_ctor)`

**Fallback — just standardize**: If the macro approach is complex, a simpler win is:
1. Fix Radios' missing `set_loading(true)` 
2. Standardize error formatting to `format!("{e:#}")` in all 6 loaders
3. Leave the structure otherwise unchanged

Start with option A; fall back to option B if the macro creates type-inference issues.

### 3. Fix Radios (required regardless of approach)

In `src/update/radios.rs:13–36`, in `handle_load_radio_stations`:
Add `self.library.radio_stations.set_loading(true);` BEFORE the `shell_task` call.

Grep for the field name: it's likely `self.library.radio_stations` but may differ.

### 4. Normalize error formatting (required regardless of approach)

Find all Err arms in the 6 loaders. Any using `e.to_string()` should use `format!("{e:#}")`.

### 5. Refactor paged views with the helper (if doing macro approach)

For Albums, Artists, Songs: replace `load_*_internal` with calls to `paginated_load!`.
The 3 wrapper fns (`handle_load_*`, `handle_*_load_page`, `force_load_*_page`) can stay
as thin wrappers, or collapse into a single `handle_load_*_page(offset: usize, force: bool)`.

### 6. Verify

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

### 7. Commit

    refactor(update): consolidate paginated library loader pattern (§3 #9)

    Albums, artists, songs each had a 58–70 line load_*_internal + 3 wrapper
    fns. Genres, playlists, radios inlined the pattern directly. All six shared
    the same set_loading(true) + shell_task structure with per-domain variation.

    [Describe: macro approach / standardization approach / what was done]

    Bug fixed: radios.rs was missing set_loading(true) before the fetch,
    risking duplicate load races on rapid navigation. Error formatting
    normalized to format!("{e:#}") across all six loaders.

    Closes audit §3 #9 (.agent/audit-progress.md §3).

Skip the Co-Authored-By trailer.

### 8. Update audit tracker

Append commit ref to `.agent/audit-progress.md` §3 row 9, flip to ✅ done.

## What NOT to touch

- `handle_albums`, `handle_artists`, `handle_genres`, etc. (the main dispatch handlers) — those prologues are a separate refactor (Lane B). Only the loader functions listed above.
- Message types, PagedBuffer implementation, AppService API methods.
- The albums credential-fetch logic (`albums_vm.get_server_config()`) — preserve it within the closure.
- Artists' `album_artists_only` flag — preserve it within the closure.

## If blocked

- If the macro approach causes type-inference failures at call sites: fall back to the
  standardization approach (fix Radios + normalize errors) and document what was done.
- If `self.library.radio_stations` doesn't exist (different field name): grep for `load_radio_stations` to find the field.
- If `PagedBuffer::set_loading` takes a bool argument: check the signature and pass `true`.

## Reporting

End with: commit ref, which approach was used (macro/standardization), whether Radios bug was fixed, LOC delta in each file, whether `cargo build` passes clean.
````
