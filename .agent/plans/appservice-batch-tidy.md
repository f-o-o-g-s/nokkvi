# AppService batch tidy-up — fanout plan (§7 #7 follow-up)

Closes the out-of-scope follow-up flagged in `31fd896` (the §7 #7 audit-tracker flip — `c4e59e3` pre-rebase): three private helpers — `load_genre_songs`, `load_playlist_songs`, `play_next_songs` — survived the AppService orchestrator split because the `*_batch` methods still call them. A fourth helper, `playback_songs`, is also a thin wrapper that becomes unused once `play_batch` routes through `QueueOrchestrator::play`. This plan extends `SongSource` with a `Batch(BatchPayload)` variant, adds `LibraryOrchestrator::resolve_batch` (collapsing the existing 30-line dispatch+dedup body), migrates the 5 in-scope batch methods on `AppService` to delegate through the orchestrators, and **deletes all four private helpers**.

Last verified baseline: **2026-05-09, `main @ 31fd896`** (Lanes A-E of `appservice-orchestrator-split.md` rebased + ff-merged into `main`; `c4e59e3` from Lane E was renumbered to `31fd896` post-rebase). All line numbers below verified against `data/src/backend/app_service.rs` at this HEAD; re-verify before starting in case `main` has moved.

Source reports: `~/nokkvi-audit-results/{monoliths-data,backend-boundary,dry-handlers,dry-api-calls}.md` and the §7 #7 plan at `.agent/plans/appservice-orchestrator-split.md` §2.4 ("Batch methods stay on AppService as today (out of scope)") — this plan retracts that scope-out.

---

## 1. Goal & rubric

The §7 #7 split left three conspicuous private helpers on `AppService` whose only remaining callers are the batch methods. Today's `AppService::resolve_batch` (lines 871-911) walks `BatchPayload.items`, dispatches each `BatchItem` variant to `albums_service.load_album_songs` / `artists_service.load_artist_songs` / `self.load_genre_songs` / `self.load_playlist_songs`, and dedups by song ID — the same dispatch the orchestrator already encodes per-entity. Hoisting it into `LibraryOrchestrator::resolve_batch` finishes the entity-dispatch unification.

Rubric (in order):
1. **Bug-class prevention.** Adding a new entity to `BatchItem` (e.g. radio station) becomes a one-arm change in the orchestrator, not a parallel-mirror update.
2. **Public AppService API stable.** `resolve_batch`, `play_batch`, `add_batch_to_queue`, `play_next_batch`, `insert_batch_at_position`, `remove_batch_from_queue` keep their names and signatures. UI handlers in `src/update/{albums,artists,genres,playlists,similar,songs,components}.rs` (22 verified call sites) **do not change**.
3. **Strictly subtractive on `AppService`.** Net delta: ~30 LOC removed, ~10 LOC added on `LibraryOrchestrator`, +1 enum variant.
4. **No semantic change.** `resolve_batch`'s skip-on-fail-and-warn behavior, dedup-by-id semantics, and "empty batch is an error" contract all preserved.

---

## 2. Architecture

Three additive moves + four method-body replacements + four helper deletions.

### 2.1 `SongSource::Batch(BatchPayload)` variant

**Location**: `data/src/types/song_source.rs` (existing file from §7 #7 Lane A).

```rust
use crate::types::{batch::BatchPayload, song::Song};

#[derive(Debug, Clone)]
pub enum SongSource {
    Album(String),
    Artist(String),
    Genre(String),
    Playlist(String),
    Preloaded(Vec<Song>),
    /// Multi-selection or context-menu batch. Resolved via
    /// `LibraryOrchestrator::resolve_batch` — flattens + dedups across
    /// per-item dispatch.
    Batch(BatchPayload),
}
```

The `BatchPayload` import is already a `data/src/types/`-local crate path; no new dependency.

### 2.2 `LibraryOrchestrator::resolve_batch`

**Location**: `data/src/backend/library_orchestrator.rs` (existing file from §7 #7 Lane A).

```rust
/// Flatten + dedup a `BatchPayload` to `Vec<Song>`. Per-item dispatch goes
/// through the per-entity `resolve_*` methods so the entity quirks (genre's
/// name-not-id, playlist's on-demand API construction) stay encapsulated.
///
/// Skip-on-fail: items that error are logged at `warn!` and dropped — matches
/// today's `AppService::resolve_batch` behavior. Empty result is a hard error.
pub async fn resolve_batch(&self, batch: BatchPayload) -> Result<Vec<Song>> {
    use std::collections::HashSet;

    use crate::types::batch::BatchItem;

    let mut resolved = Vec::new();
    let mut seen = HashSet::new();

    for item in batch.items {
        let songs_result = match item {
            BatchItem::Song(song) => Ok(vec![*song]),
            BatchItem::Album(id) => self.resolve_album(&id).await,
            BatchItem::Artist(id) => self.resolve_artist(&id).await,
            BatchItem::Genre(name) => self.resolve_genre(&name).await,
            BatchItem::Playlist(id) => self.resolve_playlist(&id).await,
        };
        match songs_result {
            Ok(songs) => {
                for song in songs {
                    if seen.insert(song.id.clone()) {
                        resolved.push(song);
                    }
                }
            }
            Err(e) => tracing::warn!("Batch item resolution failed, skipping: {e}"),
        }
    }

    if resolved.is_empty() {
        Err(anyhow::anyhow!("No songs found in batch payload"))
    } else {
        Ok(resolved)
    }
}
```

And update `LibraryOrchestrator::resolve` dispatch:

```rust
pub async fn resolve(&self, source: SongSource) -> Result<Vec<Song>> {
    match source {
        SongSource::Album(id) => self.resolve_album(&id).await,
        SongSource::Artist(id) => self.resolve_artist(&id).await,
        SongSource::Genre(name) => self.resolve_genre(&name).await,
        SongSource::Playlist(id) => self.resolve_playlist(&id).await,
        SongSource::Preloaded(songs) => Ok(songs),
        SongSource::Batch(payload) => self.resolve_batch(payload).await,
    }
}
```

The body is byte-equivalent to today's `AppService::resolve_batch` except the per-item arms now route through the existing orchestrator `resolve_album/artist/genre/playlist` methods instead of `albums_service.load_*` / `self.load_genre_songs` / etc. — that's the whole point: it removes the call-paths that keep the private helpers alive.

### 2.3 `AppService` batch method delegations

Each of the 5 in-scope batch methods becomes a 1-3 line delegation. Public API and signatures preserved verbatim.

```rust
pub async fn resolve_batch(&self, batch: BatchPayload) -> Result<Vec<Song>> {
    self.library_orchestrator().resolve_batch(batch).await
}

pub async fn play_batch(&self, batch: BatchPayload) -> Result<()> {
    let songs = self.library_orchestrator().resolve_batch(batch).await?;
    self.queue_orchestrator().play(songs, 0).await
}

pub async fn add_batch_to_queue(&self, batch: BatchPayload) -> Result<()> {
    let songs = self.library_orchestrator().resolve_batch(batch).await?;
    self.queue_orchestrator().enqueue(songs).await?;
    debug!("➕ Added batch to queue");
    Ok(())
}

pub async fn play_next_batch(&self, batch: BatchPayload) -> Result<()> {
    let songs = self.library_orchestrator().resolve_batch(batch).await?;
    self.queue_orchestrator().play_next(songs).await
}

pub async fn insert_batch_at_position(
    &self,
    batch: BatchPayload,
    position: usize,
) -> Result<()> {
    let songs = self.library_orchestrator().resolve_batch(batch).await?;
    self.queue_orchestrator().insert_at(songs, position).await?;
    debug!("📌 Inserted batch at queue position {}", position);
    Ok(())
}
```

`remove_batch_from_queue` (line 959) consumes `Vec<usize>` indices and **stays unchanged** — it doesn't fit the orchestrator grid.

### 2.4 Private helper deletions

After §2.3 lands, all four of these become unreferenced:

| Helper | Line (`main @ 31fd896`) | Why dead |
|---|---:|---|
| `load_genre_songs` | 758 | Only called by `resolve_batch` arm; that arm now uses `library_orchestrator().resolve_genre`. |
| `load_playlist_songs` | 773 | Same. |
| `play_next_songs` | 793 | Only called by `play_next_batch`; that method now uses `queue_orchestrator().play_next`. |
| `playback_songs` | 921 | Only called by `play_batch`; that method now uses `queue_orchestrator().play`. |

Verify zero remaining callers via grep before deletion (step 8 in the lane prompt).

---

## 3. Lane decomposition

Single lane. The work is naturally sequential (orchestrator additions must precede `AppService` migrations must precede helper deletions), and at ~30 LOC delta the multi-worktree overhead doesn't pay back. Sub-agent dispatch inside the lane parallelizes the verification + drafting steps.

| Lane | Scope | Files touched | Commit count (est.) | Effort | Depends on |
|---|---|---|---:|---|---|
| **A** (batch tidy) | `SongSource::Batch` variant + `LibraryOrchestrator::resolve_batch` + 5 AppService delegations + 4 helper deletions | `data/src/types/song_source.rs`, `data/src/backend/library_orchestrator.rs`, `data/src/backend/app_service.rs`, possibly `.agent/audit-progress.md` (out-of-scope-follow-up note removal) | 4-6 | S | §7 #7 Lanes A-E merged to `main` |

**Hard prerequisite**: §7 #7 fanout (Lanes A-E of `appservice-orchestrator-split.md`) must be merged to `main` first. The lane prompt verifies this via `git log` + file existence check before doing any work.

---

## 4. Per-lane scope (callers verified on `main @ 31fd896`)

### Lane A — batch tidy

**Files**:
- `data/src/types/song_source.rs` — add `Batch(BatchPayload)` variant + import.
- `data/src/backend/library_orchestrator.rs` — add `resolve_batch` method + dispatch arm in `resolve`.
- `data/src/backend/app_service.rs` — replace 5 batch method bodies; delete 4 private helpers; remove the now-redundant `BatchItem` import / `HashSet` use that survived inside `resolve_batch`.
- `.agent/audit-progress.md` — final commit clears the out-of-scope-follow-up note in §7 #7's row text (or rewrites it to "all callers migrated by `<this commit>`").

**Sites to migrate** (5 method bodies + 4 helper deletions, plus the orchestrator additions):

| Type | Item | Action |
|---|---|---|
| Add | `SongSource::Batch(BatchPayload)` variant | Insert in `data/src/types/song_source.rs`. |
| Add | `LibraryOrchestrator::resolve_batch` | Insert in `data/src/backend/library_orchestrator.rs` (body per §2.2). |
| Add | `LibraryOrchestrator::resolve` dispatch arm for `Batch` | Same file. |
| Migrate | `AppService::resolve_batch` (line 871) | 1-line delegation. |
| Migrate | `AppService::play_batch` (line 915) | 2-line delegation through `library` + `queue.play`. |
| Migrate | `AppService::add_batch_to_queue` (line 932) | 3-line delegation through `library` + `queue.enqueue` + debug log. |
| Migrate | `AppService::play_next_batch` (line 940) | 2-line delegation through `library` + `queue.play_next`. |
| Migrate | `AppService::insert_batch_at_position` (line 946) | 3-line delegation through `library` + `queue.insert_at` + debug log. |
| **Skip** | `AppService::remove_batch_from_queue` (line 959) | Unchanged — consumes indices, not a `BatchPayload`. |
| Delete | `AppService::load_genre_songs` (line 758) | Verify zero callers post-migration, then delete. |
| Delete | `AppService::load_playlist_songs` (line 773) | Same. |
| Delete | `AppService::play_next_songs` (line 793) | Same. |
| Delete | `AppService::playback_songs` (line 921) | Same. |

**External callers preserved** (verified via `rg "shell\.(play_batch|add_batch_to_queue|play_next_batch|insert_batch_at_position|remove_batch_from_queue|resolve_batch)" src/`):
- `src/update/albums.rs:634, 641, 652, 826` — `insert_batch_at_position`, `add_batch_to_queue`, `play_batch`, `play_next_batch`.
- `src/update/artists.rs:476, 483, 615` — `insert_batch_at_position`, `add_batch_to_queue`, `play_next_batch`.
- `src/update/genres.rs:305, 312, 495` — `insert_batch_at_position`, `add_batch_to_queue`, `play_next_batch`.
- `src/update/playlists.rs:219, 226, 380` — `insert_batch_at_position`, `add_batch_to_queue`, `play_next_batch`.
- `src/update/similar.rs:38, 45, 55` — `insert_batch_at_position`, `add_batch_to_queue`, `play_batch`.
- `src/update/songs.rs:500, 507, 580, 591` — `insert_batch_at_position`, `add_batch_to_queue`, `play_next_batch`, `play_batch`.
- `src/update/components.rs:901` — `resolve_batch` (the only direct caller; bare `resolve_batch` is preserved as a public 1-line delegation specifically because of this site).

22 call sites total. None change shape.

---

## 5. Verification

Run after each commit slice:

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass before pushing.

**Lane A specific checks**:
- `grep -c 'fn load_genre_songs\|fn load_playlist_songs\|fn play_next_songs\|fn playback_songs' data/src/backend/app_service.rs` should equal `0` post-deletion.
- `grep -c 'SongSource::Batch' data/src/types/song_source.rs` should equal `1` (variant declaration).
- `grep -c 'fn resolve_batch' data/src/backend/library_orchestrator.rs` should equal `1`.
- `grep -c 'fn resolve_batch\|fn play_batch\|fn add_batch_to_queue\|fn play_next_batch\|fn insert_batch_at_position\|fn remove_batch_from_queue' data/src/backend/app_service.rs` should equal `6` (all public batch methods preserved by name).
- `wc -l data/src/backend/app_service.rs` delta: ~30-40 LOC reduction.
- `cargo test` count unchanged from baseline.

---

## 6. What this does NOT do

- **No public AppService API rename or removal.** All 6 batch method names + signatures preserved (5 migrated, `remove_batch_from_queue` untouched).
- **No new dependency.** Plan uses existing `BatchPayload`/`BatchItem` types from `data/src/types/batch.rs`.
- **No behavior change.** Skip-on-fail logging, dedup-by-id, empty-batch error semantics, and the two `debug!` log lines all preserved.
- **No `remove_batch_from_queue` migration.** Consumes `Vec<usize>` not `SongSource`; stays as today.
- **No UI handler changes.** Zero edits to `src/update/*`.
- **No new orchestrator method on `QueueOrchestrator`.** This plan only adds `resolve_batch` to `LibraryOrchestrator`; queue verbs (play/enqueue/play_next/insert_at) are reused from §7 #7.
- **No reformatting outside touched files.**
- **No `.agent/rules/` updates.** Lane A may touch `.agent/audit-progress.md` to clear the follow-up note; rules-doc syncing is `/sync-rules`'s job.

---

## Fanout Prompts

### lane-a-batch

worktree: ~/nokkvi-batch-tidy
branch: refactor/appservice-batch-tidy
effort: max
permission-mode: bypassPermissions

````
Task: extend `SongSource` with a `Batch(BatchPayload)` variant, add `LibraryOrchestrator::resolve_batch`, migrate the 5 in-scope batch methods on `AppService` to delegate through the orchestrators, and DELETE 4 now-unreferenced private helpers (`load_genre_songs`, `load_playlist_songs`, `play_next_songs`, `playback_songs`).

Plan doc: /home/foogs/nokkvi/.agent/plans/appservice-batch-tidy.md (sections 2.1-2.4, 4 "Lane A").

Working directory: ~/nokkvi-batch-tidy (this worktree). Branch: refactor/appservice-batch-tidy. The worktree is already created — do NOT run `git worktree add`.

## Hard prerequisite

§7 #7 (AppService orchestrator split, `.agent/plans/appservice-orchestrator-split.md`) Lanes A-E must be merged to `main` before this lane can compile. Verify:

```bash
git fetch origin main
git log origin/main --oneline | grep 'appservice-orchestrator-split\|§7 #7' | head -5
ls data/src/types/song_source.rs data/src/backend/library_orchestrator.rs data/src/backend/queue_orchestrator.rs
```

All three files must exist on `main` and the audit-tracker doc-flip commit (`31fd896` or descendant) must be reachable. If not: STOP. The §7 #7 fanout hasn't merged yet; this plan can't run.

If `main` is up to date, rebase your branch onto it: `git rebase origin/main`.

## What to do

### 1. Verify baseline + state

- `git log -1 --oneline` should be on or after `31fd896` (the §7 #7 audit-tracker flip).
- `wc -l data/src/backend/app_service.rs` — capture the baseline number (1037 LOC at `31fd896`); you'll compare post-tidy.
- `grep -n 'fn load_genre_songs\|fn load_playlist_songs\|fn play_next_songs\|fn playback_songs' data/src/backend/app_service.rs` should list 4 helpers (line numbers around 758/773/793/921 at `31fd896`; may have drifted if main has moved).
- `grep -n 'fn resolve_batch\|fn play_batch\|fn add_batch_to_queue\|fn play_next_batch\|fn insert_batch_at_position\|fn remove_batch_from_queue' data/src/backend/app_service.rs` should list 6 methods (around 871/915/932/940/946/959).

### 2. Sub-agent dispatch — confirm BatchPayload + draft method bodies

Issue ONE general-purpose sub-agent to draft the migration patches:

```
Agent({
  description: "Draft batch tidy migration patches",
  subagent_type: "general-purpose",
  prompt: "Read /home/foogs/nokkvi/data/src/types/batch.rs in full and /home/foogs/nokkvi/data/src/backend/app_service.rs lines 850-945. Then produce:

  1. The exact new body for `LibraryOrchestrator::resolve_batch(batch: BatchPayload) -> Result<Vec<Song>>` that mirrors the existing `AppService::resolve_batch` (lines 871-911) but routes per-item arms through `self.resolve_album/resolve_artist/resolve_genre/resolve_playlist`. Preserve the skip-on-fail-warn semantics and the dedup-by-id logic. Include all required imports.

  2. The updated `LibraryOrchestrator::resolve` dispatch with the new `SongSource::Batch(payload) => self.resolve_batch(payload).await` arm.

  3. The exact new body for each of these 5 AppService methods, as 1-3 line delegations:
     - resolve_batch (line 871) — 1-line: library_orchestrator().resolve_batch(batch).await
     - play_batch (line 915) — resolve_batch + queue_orchestrator().play(songs, 0).
     - add_batch_to_queue (line 932) — resolve_batch + queue_orchestrator().enqueue(songs) + preserve the debug! log line.
     - play_next_batch (line 940) — resolve_batch + queue_orchestrator().play_next(songs).
     - insert_batch_at_position (line 946) — resolve_batch + queue_orchestrator().insert_at(songs, position) + preserve the debug! log line.

  4. Confirm by grep that the 4 private helpers (load_genre_songs, load_playlist_songs, play_next_songs, playback_songs) have ZERO remaining callers in app_service.rs once the 5 migrations above are applied. If any caller survives outside these 5 methods, flag it with file:line — that caller must be migrated first or the helper kept.

  5. Note any imports that become unused after the migration (e.g. `use crate::types::batch::BatchItem;` inside the old AppService::resolve_batch body — moves to the orchestrator file). List them so they can be removed.

  Output as a structured report under 1000 words. Format the new bodies in Rust code fences."
})
```

Wait for the sub-agent. Cross-check its proposed bodies against the actual current code before applying — if any existing tracing/debug/side-effect is missing from a proposed body, flag and adjust.

### 3. Apply: Add `SongSource::Batch` variant

Edit `data/src/types/song_source.rs`. Add the `Batch(BatchPayload)` variant per plan §2.1, plus the `BatchPayload` import. The existing variants stay in the same order; `Batch` is appended last.

Verify: `cargo build`. (Build only — no tests yet; the new variant has no consumers.)

Commit slice 1: `feat(types): add SongSource::Batch variant for multi-selection dispatch`.

### 4. Apply: Add `LibraryOrchestrator::resolve_batch` + dispatch arm

Edit `data/src/backend/library_orchestrator.rs`. Add the `resolve_batch` method per the sub-agent's draft, plus the `Batch` arm in `resolve()`. Add necessary imports (`std::collections::HashSet`, `crate::types::batch::{BatchItem, BatchPayload}`).

Verify: `cargo build && cargo test -p nokkvi-data library_orchestrator::`. The 5+ existing tests must pass; you may add a new `resolve_batch_dedups_and_skips_failures` test if mocking permits — otherwise compile-only smoke is acceptable (instantiate with empty `BatchPayload::new()` and assert the empty-batch error path).

Commit slice 2: `feat(library_orchestrator): add resolve_batch with per-item dispatch + dedup`. Note in the commit body that this hoists the body of `AppService::resolve_batch` and routes each `BatchItem` arm through the orchestrator's per-entity resolvers.

### 5. Apply: Migrate the 5 AppService batch methods

Edit `data/src/backend/app_service.rs`. Use the Edit tool one method at a time, in this order:

a. `resolve_batch` (line ~871) — 1-line delegation. Build.
b. `play_batch` (line ~915) — 2-line delegation. Build.
c. `add_batch_to_queue` (line ~932) — preserve the `debug!` log. Build.
d. `play_next_batch` (line ~940) — 2-line delegation. Build.
e. `insert_batch_at_position` (line ~946) — preserve the `debug!` log. Build.

After each method: `cargo build` to catch type errors immediately. If a build fails, undo that one edit, investigate, fix, re-apply.

After all 5 are migrated: `cargo test && cargo clippy --all-targets -- -D warnings && cargo +nightly fmt --all -- --check`.

Commit slice 3: `refactor(app_service): migrate 5 batch methods to LibraryOrchestrator + QueueOrchestrator delegation`. Mention all 5 method names in the commit body.

### 6. Apply: Verify the 4 private helpers are now dead

```bash
rg -n 'load_genre_songs|load_playlist_songs|play_next_songs|playback_songs' data/src/backend/app_service.rs
```

Expected output: ONLY the function definitions themselves (the `async fn ...` lines). If any other line references one of these helpers, STOP — that caller must be migrated first.

Also grep the wider data crate: `rg -n 'load_genre_songs|load_playlist_songs|play_next_songs|playback_songs' data/src/`. Expected: only the 4 definitions in `app_service.rs`.

### 7. Apply: Delete the 4 helpers

Use the Edit tool to delete each helper body. Watch for the `use std::collections::HashSet` and `use crate::types::batch::BatchItem;` imports that previously lived inside `AppService::resolve_batch` — those have already moved to the orchestrator file in step 4 and any leftover at the top of `app_service.rs` should be removed.

After deletion: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo +nightly fmt --all -- --check`.

Commit slice 4: `refactor(app_service): delete 4 obsolete private helpers (load_genre_songs / load_playlist_songs / play_next_songs / playback_songs)`. Note in the commit body that these were retained by the §7 #7 split as a documented out-of-scope follow-up; this commit closes that follow-up.

### 8. Update audit tracker

Edit `.agent/audit-progress.md` — locate §7 row 7 (the AppService LibraryOrchestrator + QueueOrchestrator split row) and either:

a. Edit the existing row's evidence line to remove the "Out-of-scope follow-up: private helpers retained pending..." note, replacing it with "All four private helpers (load_genre_songs / load_playlist_songs / play_next_songs / playback_songs) deleted by `<commit ref of slice 4>` after batch tidy lands." (preferred — keeps the row history coherent), OR

b. Add a new row to the existing follow-up tracking section if the doc has one.

Pick (a). Match the format used by sibling ✅ rows (commit refs in monospace, evidence sentence complete).

Commit slice 5: `docs(audit): close §7 #7 batch follow-up after private helper deletions land`.

### 9. Verify (final)

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass.

Final structural checks:
- `grep -c 'fn load_genre_songs\|fn load_playlist_songs\|fn play_next_songs\|fn playback_songs' data/src/backend/app_service.rs` returns `0`.
- `grep -c 'SongSource::Batch' data/src/types/song_source.rs` returns `1`.
- `grep -c 'fn resolve_batch' data/src/backend/library_orchestrator.rs` returns `1`.
- `grep -c 'fn resolve_batch\|fn play_batch\|fn add_batch_to_queue\|fn play_next_batch\|fn insert_batch_at_position\|fn remove_batch_from_queue' data/src/backend/app_service.rs` returns `6` (all 6 public batch methods preserved by name; 5 migrated + remove_batch_from_queue untouched).
- `wc -l data/src/backend/app_service.rs` delta from baseline: ~30-40 LOC reduction expected.

### 10. Report

End with: commits (refs + subjects), `wc -l data/src/backend/app_service.rs` baseline → final delta, the 4 deleted helper line numbers (from baseline), and one sentence on whether the sub-agent's draft matched verbatim or required adjustment.

## What NOT to touch

- `remove_batch_from_queue` (line ~959) — out of scope; consumes `Vec<usize>` indices, not a `BatchPayload`.
- `QueueOrchestrator` — this plan does not add new methods; reuses the existing 5 verbs from §7 #7 Lane B.
- Any UI call site (`src/update/*`) — public AppService API unchanged.
- `data/src/types/batch.rs` — `BatchPayload` / `BatchItem` definitions are untouched (they're the input type for the new orchestrator method).
- Any `play_*` / `add_*` / `insert_*` / `play_next_*` entity×verb method that's NOT a batch method — those landed in §7 #7 Lanes C/D and are stable.
- `.agent/rules/` files.
- The `*_song_by_id` methods (out of scope — bespoke find-by-id logic).

## If blocked

- If the sub-agent's draft of `LibraryOrchestrator::resolve_batch` includes a subtle behavior change (e.g. uses `?` propagation instead of skip-on-fail-warn): STOP, report the diff, fix to match today's `AppService::resolve_batch` skip-on-fail behavior verbatim. The audit-tracker flip explicitly cites "all callers migrated" — semantic divergence breaks that.
- If a 5th caller of one of the 4 private helpers exists outside the 5 batch methods (e.g. some util in `data/src/services/`): STOP, list it, ask. Do not delete a helper that still has external callers.
- If `cargo clippy` flags `dead_code` on `LibraryOrchestrator::resolve_batch` because no caller routes through `SongSource::Batch` directly: that's expected — the direct callers go through `library_orchestrator().resolve_batch(batch)` not through the enum. Add `#[allow(dead_code)]` on the dispatch arm in `resolve()` only if clippy specifically demands it — otherwise leave as-is.
- If `cargo test` regresses a test that exercised the old `AppService::resolve_batch` skip-on-fail behavior: re-read the failing test; the new orchestrator body must replicate today's behavior exactly (warn-and-continue per item, hard error on fully-empty result).
- If a UI call site at `src/update/components.rs:901` (the bare `shell.resolve_batch(...)` caller) regresses: this is the only direct caller of the AppService-level `resolve_batch` outside the other 4 batch methods. The 1-line delegation in step 5a preserves it; if the call site fails, inspect the orchestrator dispatch path before editing the call site.
````
