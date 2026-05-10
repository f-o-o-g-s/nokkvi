# Queue type-level invariants — fanout plan (IG-1 + IG-2 + IG-4 + IG-5)

Closes `.agent/audit-progress.md` §7 #12. Picks up the highest-leverage invariant-gap cluster in the project: queue mutations and queue navigation are enforced by comments today; this plan makes them enforced by types.

Last verified baseline: **2026-05-08, `main @ HEAD = 3ef2cf2`**.

---

## 1. Goal & rubric

The queue is the most agent-bug-prone surface in nokkvi. Today four discrete invariants are doc-only:

| ID | Doc-only contract | Today's enforcement | Failure mode |
|----|---|---|---|
| **IG-1** | "Don't mutate Queue fields directly" | nothing — `get_queue_mut() -> &mut Queue` is public | Future mutator drifts `pool`/`order`/`queued` out of sync |
| **IG-2** | `insert_song_at` (singular) sets `current_index`; `insert_songs_at` (plural) doesn't | near-identical names | Caller picks the wrong arity, playhead jumps silently |
| **IG-4** | `peek_next_song` → `transition_to_queued` is the only valid pairing | docstrings | Caller transitions without a peek (silent no-op) or peeks then `set_current_index` (state corruption) |
| **IG-5** | Every queue mutation must call `clear_queued()` | 11 explicit call sites in `queue/mod.rs` | New mutator forgets the call — navigator transitions to a stale/wrong song |

Rubric (in order): (1) prevent the bug class outright, (2) keep the public API ergonomic, (3) minimal blast radius into the UI crate, (4) the test suite passes unchanged after each lane unless the lane explicitly migrates a test for ergonomics.

---

## 2. Architecture

Two new typestate guards plus one rename. All three live entirely inside `data/src/services/queue/`. The UI crate is unaffected.

### 2.1 `PeekedQueue<'a>` (closes IG-4)

**File**: new section in `data/src/services/queue/navigation.rs`.

```rust
pub struct PeekedQueue<'a> {
    mgr: Option<&'a mut QueueManager>,
    info: NextSongResult,
}

impl<'a> PeekedQueue<'a> {
    pub fn song(&self) -> &Song { &self.info.song }
    pub fn index(&self) -> usize { self.info.index }
    pub fn reason(&self) -> &str { &self.info.reason }
    pub fn info(&self) -> &NextSongResult { &self.info }

    /// Consume the peek and advance current_index/current_order.
    /// The only public way to commit a peek to a transition.
    pub fn transition(mut self) -> TransitionResult {
        let mgr = self.mgr.take().expect("guard already consumed");
        mgr.transition_to_queued_internal()
            .expect("PeekedQueue invariant: queued is set when guard exists")
    }
}

impl<'a> Drop for PeekedQueue<'a> {
    fn drop(&mut self) {
        // Backend-boundary §3 IG-4: "drops the guard without transitioning
        // gets the implicit clear_queued() for free." Peek-then-drop is the
        // 'abandon' path — turn it into a clean reset rather than silent
        // stale-queued state.
        if let Some(mgr) = self.mgr.take() {
            mgr.clear_queued();
        }
    }
}

impl QueueManager {
    /// Peek at the next song. Returns a guard that owns the only path
    /// to `transition()`. Dropping the guard without `transition()`
    /// clears the queued state.
    pub fn peek_next_song(&mut self) -> Option<PeekedQueue<'_>>;

    /// Internal transition kept for `get_next_song`'s use only.
    pub(crate) fn transition_to_queued_internal(&mut self) -> Option<TransitionResult>;
}
```

**Behavior change vs today**: peek-without-transition currently leaves `queued` set; under the guard it clears it. The `decide_transition` flow in `data/src/services/playback.rs:321-324` already defends against `queued` being cleared concurrently and re-peeks — so this change makes the existing defensive path the unconditional path. Re-peek is idempotent (current `peek_next_song` recomputes from `current_order + 1`).

**Removed from public API**: `pub fn transition_to_queued`. Renamed to `pub(crate) fn transition_to_queued_internal` so `navigation::get_next_song` (peek-then-transition convenience for manual skip) can still call it directly without paying the guard ceremony.

### 2.2 `QueueWriteGuard<'a>` (closes IG-5)

**File**: new sibling submodule `data/src/services/queue/write_guard.rs` (or inline in `mod.rs` — implementer's call).

```rust
pub struct QueueWriteGuard<'a> {
    mgr: Option<&'a mut QueueManager>,
}

impl<'a> Drop for QueueWriteGuard<'a> {
    /// Safety net for early-return paths (`?` propagating, panics).
    /// `commit_*` methods call this first via `take()`, so a normal
    /// commit doesn't double-clear.
    fn drop(&mut self) {
        if let Some(mgr) = self.mgr.take() {
            mgr.clear_queued();
        }
    }
}

impl<'a> QueueWriteGuard<'a> {
    /// Commit with full save (queue + song pool). Use after add/remove/insert/set_queue.
    pub fn commit_save_all(mut self) -> Result<()> {
        let mgr = self.mgr.take().expect("guard already consumed");
        mgr.clear_queued();
        mgr.save_all()
    }

    /// Commit with order-only save. Use after move/sort/shuffle/mode-toggle.
    pub fn commit_save_order(mut self) -> Result<()> {
        let mgr = self.mgr.take().expect("guard already consumed");
        mgr.clear_queued();
        mgr.save_order()
    }

    /// Commit without persisting. Use after set_current_index (in-memory only).
    pub fn commit_no_save(mut self) {
        let mgr = self.mgr.take().expect("guard already consumed");
        mgr.clear_queued();
    }
}

impl<'a> std::ops::Deref for QueueWriteGuard<'a> {
    type Target = QueueManager;
    fn deref(&self) -> &QueueManager {
        self.mgr.as_deref().expect("guard already consumed")
    }
}

impl<'a> std::ops::DerefMut for QueueWriteGuard<'a> {
    fn deref_mut(&mut self) -> &mut QueueManager {
        self.mgr.as_deref_mut().expect("guard already consumed")
    }
}

impl QueueManager {
    fn write(&mut self) -> QueueWriteGuard<'_> {
        QueueWriteGuard { mgr: Some(self) }
    }
}
```

**Why three named commit methods, not one**: the current 13 mutation methods split cleanly into three save modes (full / order-only / no-save). Naming each one means a new mutator's author has to *choose* a save mode out loud rather than guess what `commit()` does. This is the agent-friendly shape — exhaustive, named, hard to drift.

**Why save errors propagate (Drop is clear-only, not save-too)**: `save_all` writes redb. A swallowed save error means the in-memory queue diverges from disk silently — exactly the disk-corruption class. `Result` propagation is non-negotiable. Drop runs `clear_queued` only, which is infallible. The synthesis report's "Drop calls clear_queued() AND save_all()" is interpreted here as "Drop is the safety net for the two invariants" — for save we satisfy the safety net via the `commit_save_*()` methods that call `clear_queued()` *before* `save_all()`, so even if save fails, the in-memory navigator state is consistent (queued cleared) and the error reaches the caller.

### 2.3 `insert_song_and_make_current` rename (closes IG-2)

```rust
/// Insert a song at `index` and set it as the currently-playing song.
/// Used to re-insert songs from history (consume mode).
///
/// Delegates to `insert_songs_at` + `set_current_index` so the
/// "make current" semantics are explicit rather than buried inside
/// a near-identically-named method.
pub fn insert_song_and_make_current(&mut self, index: usize, song: Song) -> Result<()> {
    let clamped = index.min(self.queue.song_ids.len());
    self.insert_songs_at(clamped, vec![song])?;
    self.set_current_index(Some(clamped));
    self.save_order()
}
```

The delegation reads as one atomic "insert-and-jump" rather than mirroring `insert_songs_at`'s body with a one-line difference.

### 2.4 `get_queue_mut` deletion (closes IG-1)

Verified: zero call sites outside the definition (`grep -rn '\.get_queue_mut(' --include='*.rs'` returns only the `pub fn` line). Pure deletion.

---

## 3. Lane decomposition (parallel)

Three independent lanes, no required ordering:

| Lane | Scope | Files touched | Commit count (est.) | Effort |
|---|---|---|---:|---|
| **A** (rename + escape-hatch) | IG-1 + IG-2 | `queue/mod.rs`, `services/playback.rs` | 1–2 | S |
| **B** (peeked guard) | IG-4 | `queue/navigation.rs`, `queue/mod.rs` (tests), `services/playback.rs`, `backend/playback_controller.rs` | 3–5 | M |
| **C** (write guard) | IG-5 | `queue/write_guard.rs` (new), `queue/mod.rs`, `queue/order.rs` (visibility), `queue/mod.rs` (tests for mode-toggles) | 4–6 | M-L |

**Conflict zones**:
- Lane A renames `insert_song_at`. Lane C wraps that method's body in a guard. If A merges first, C rebases trivially (renamed signature). If C merges first, A's rename is a one-line edit on the wrapped body. Either way: ~2-line rebase.
- Lane B and Lane C touch the test module in `queue/mod.rs`. Lane B migrates `qm.peek_next_song().unwrap()` → guard accessors. Lane C does not change test bodies (mutator API stays the same). No overlap.
- Lane B touches `services/playback.rs` (peek/transition call sites). Lane A also touches it (one `insert_song_at` rename). Different lines, no conflict.

**Recommended merge order**: A → B → C (smallest to largest blast radius). Acceptable to merge B and C in either order after A.

---

## 4. Per-lane scope (callers verified at baseline)

### Lane A — files & sites

- `data/src/services/queue/mod.rs:404` — delete `pub fn get_queue_mut`.
- `data/src/services/queue/mod.rs:502` — rename `insert_song_at` → `insert_song_and_make_current` and rewrite body as delegation.
- `data/src/services/playback.rs:515` — update sole external call: `queue_manager.insert_song_at(insert_idx, song.clone())?;` → `queue_manager.insert_song_and_make_current(insert_idx, song.clone())?;`.

### Lane B — files & sites

Internal-to-`data/`. UI crate untouched.

- `data/src/services/queue/navigation.rs:11` — add `PeekedQueue<'a>` struct + impls.
- `data/src/services/queue/navigation.rs:47` — change `peek_next_song` signature; package the existing `NextSongResult` body into a `PeekedQueue` constructor.
- `data/src/services/queue/navigation.rs:146` — rename `transition_to_queued` → `transition_to_queued_internal`, mark `pub(crate)`.
- `data/src/services/queue/navigation.rs:218,227` — update internal `get_next_song` to use `transition_to_queued_internal` directly (it already has the queued-set invariant from its own `peek_next_song` call inside).
- `data/src/services/playback.rs:300, 322-324, 325` — migrate to guard pattern (peek captures song, drops; transition via fresh peek's `.transition()`).
- `data/src/backend/playback_controller.rs:521` — gapless prep migrates to `peek().song().clone(); drop(peeked);` shape.
- Tests in `data/src/services/queue/mod.rs` (lines 858–940) — update `qm.peek_next_song().unwrap()` and `qm.transition_to_queued()` to use guard accessors.
- Tests in `data/src/services/queue/navigation.rs` (lines 313–520) — same migration.
- Proptest in `navigation.rs` (line 1100 `peek_is_idempotent`) — assert idempotency by holding/dropping guards.

### Lane C — files & sites

- New file `data/src/services/queue/write_guard.rs` (or inline in `mod.rs`) — `QueueWriteGuard<'a>` + impls.
- `data/src/services/queue/mod.rs` — add `fn write(&mut self) -> QueueWriteGuard<'_>` private helper.
- `data/src/services/queue/order.rs:76` — narrow `clear_queued` from `pub(crate)` to `pub(super)` if all external callers go through the guard. (Verify; if `playback.rs:262` or `navigation.rs:261,283` call it directly, leave as `pub(crate)`.)
- Wrap each mutation method in `mod.rs`:
  | Line | Method | Save mode |
  |---:|---|---|
  | 129 | `add_songs` | `commit_save_all` |
  | 147 | `set_queue` | `commit_save_all` |
  | 170 | `remove_song` | `commit_save_all` (conditional) |
  | 231 | `toggle_shuffle` | `commit_save_order` |
  | 249 | `shuffle_queue` | `commit_save_order` |
  | 284 | `sort_queue` | `commit_save_order` |
  | **354** | **`set_repeat`** | **`commit_save_order`** (NEW: now clears queued) |
  | **360** | **`toggle_consume`** | **`commit_save_order`** (NEW: now clears queued) |
  | 411 | `set_current_index` | `commit_no_save` |
  | 424 | `move_item` | `commit_save_order` |
  | 467 | `insert_after_current` | `commit_save_all` |
  | 502 | `insert_song_and_make_current` (post-Lane A) | delegated; no direct guard |
  | 521 | `insert_songs_at` | `commit_save_all` |

- Add tests asserting `set_repeat` and `toggle_consume` now clear `queue.queued` (mirrors existing `queue_mutation_clears_queued` at line 923).

**Behavior change**: `set_repeat` and `toggle_consume` previously did NOT clear `queued`. Under the guard they do. This is a deliberate alignment with `toggle_shuffle` — switching repeat-track on while a different song is queued would otherwise transition to the wrong song. This closes the queue-side half of IG-3 (the engine-side `reset_next_track()` half remains a separate audit item).

---

## 5. Verification (every lane)

Run after each commit slice:

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass before pushing the slice. Per-lane TDD is light because the changes are structural (no behavior changes except set_repeat/toggle_consume in Lane C, which has explicit new tests).

---

## 6. What each lane does NOT do

- No UI crate edits. The UI calls `queue_service.insert_songs_at(...)` and friends through `backend/queue.rs`; those wrappers are stable.
- No new dependencies (per `code-standards.md`).
- No reformatting outside touched files.
- No drive-by docstring rewrites unrelated to the typestate.
- No engine-side changes (IG-3's `reset_next_track()` enforcement is out of scope; the audit lists it separately).
- No `set_current_index` rename (audit IG-9 — `jump_to_index_for_play_from_here`); that's a separate item.
- Lane B does not introduce `prepare_next_for_gapless` or any other parallel "peek without guard" API. The single `peek_next_song -> Option<PeekedQueue<'_>>` is the only public peek path.
- Lane C does not introduce a `commit()` shorthand — the three named methods are the API.

---

## Fanout Prompts

### lane-a-rename

worktree: ~/nokkvi-queue-igs-a
branch: refactor/queue-igs-rename
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane A of the queue type-level invariants plan.

Plan doc: /home/foogs/nokkvi/.agent/plans/queue-typestate-igs.md (sections 2.3, 2.4, 4 "Lane A").

Scope: close IG-1 (delete `get_queue_mut`) and IG-2 (rename `insert_song_at` → `insert_song_and_make_current`, rewrite body as delegation). One PR, one or two commits.

Working directory: ~/nokkvi-queue-igs-a (this worktree). Branch: refactor/queue-igs-rename. The worktree is already created — do NOT run `git worktree add`.

## What to do

1. Verify the baseline before editing:
   - `git log -1 --oneline` should show `3ef2cf2` or a descendant on `main`.
   - `grep -rn '\.get_queue_mut(' --include='*.rs' .` must return zero matches outside the definition. If it returns hits, STOP and ask — the plan's "no callers" assumption is wrong.
   - `grep -rn '\.insert_song_at(' --include='*.rs' .` should return exactly ONE call site (`data/src/services/playback.rs:515`) plus the definition and tests.

2. Delete `pub fn get_queue_mut` at `data/src/services/queue/mod.rs:404` (4 lines including the doc comment if any).

3. Rename `pub fn insert_song_at` (line 502) to `pub fn insert_song_and_make_current` and rewrite body as delegation per plan §2.3:
   ```rust
   /// Insert a song at `index` and set it as the currently-playing song.
   /// Used to re-insert songs from history (consume mode).
   pub fn insert_song_and_make_current(&mut self, index: usize, song: Song) -> Result<()> {
       let clamped = index.min(self.queue.song_ids.len());
       self.insert_songs_at(clamped, vec![song])?;
       self.set_current_index(Some(clamped));
       self.save_order()
   }
   ```
   Note: `insert_songs_at` already calls `clear_queued` + `save_all` internally. `set_current_index` clears queued. The trailing `save_order()?` persists the index change set by `set_current_index`.

4. Update the sole caller at `data/src/services/playback.rs:515`:
   `queue_manager.insert_song_at(insert_idx, song.clone())?;` → `queue_manager.insert_song_and_make_current(insert_idx, song.clone())?;`.

5. The doc comment on `insert_songs_at` (line 517-520) should be tightened to point at the new singular's name, since the name no longer collides:
   - Current: "Used for cross-pane drag-and-drop (browsing panel → queue at drop position). Does NOT change `current_index` to point at the inserted songs..."
   - Keep as-is, but consider adding a one-line cross-ref: "See `insert_song_and_make_current` for the singular variant that sets the playhead."

6. Verify in this order, fixing any failure before continuing:
   ```
   cargo build
   cargo test
   cargo clippy --all-targets -- -D warnings
   cargo +nightly fmt --all -- --check
   ```

7. Commit. Use the conventional-commits format from `~/nokkvi/CLAUDE.md`. Suggested message:

       refactor(queue): close IG-1, IG-2 — drop get_queue_mut, rename insert singular

       - Delete unused `get_queue_mut` (escape hatch with zero callers).
       - Rename `insert_song_at` to `insert_song_and_make_current` to make
         the playhead-jumping semantics explicit; near-identical name with
         `insert_songs_at` was an audit footgun (§6 IG-2).
       - Refactor the new singular as a delegation to `insert_songs_at` +
         `set_current_index` so the body is no longer a parallel mirror.

       Closes part of `.agent/audit-progress.md` §7 #12 (IG-1 + IG-2).

   Skip the `Co-Authored-By` trailer per global instructions.

8. After the commit lands, append the commit ref to `.agent/audit-progress.md` §7 row 12 — but DO NOT mark the row done, only add the partial ref. Lanes B and C close the rest.

## What NOT to touch

- Anything in `data/src/services/queue/navigation.rs` (Lane B's territory).
- Anything in any other mutation method body (Lane C's territory).
- The UI crate (`src/`).
- `.agent/rules/` files.
- Any other audit item.

## If blocked

- If `cargo test` fails with an unrelated test failure on baseline: stop, report, do not proceed.
- If a caller of `insert_song_at` exists that you didn't expect: stop, list it, ask before continuing.
- If clippy flags the delegation form (e.g., `unused_self`): adjust minimally; do not paper over with `#[allow]`.

## Reporting

End with a short summary: which commit(s), which files changed, line counts. No hand-wave summaries.
````

### lane-b-peeked-guard

worktree: ~/nokkvi-queue-igs-b
branch: refactor/queue-igs-peek
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane B of the queue type-level invariants plan — close IG-4 (peek/transition typestate).

Plan doc: /home/foogs/nokkvi/.agent/plans/queue-typestate-igs.md (section 2.1 for the design, section 4 "Lane B" for the file inventory).

Working directory: ~/nokkvi-queue-igs-b (this worktree). Branch: refactor/queue-igs-peek. The worktree is already created — do NOT run `git worktree add`.

## What to do

Implement `PeekedQueue<'a>` exactly as specified in plan §2.1 and migrate every call site.

### 1. Verify baseline

- `git log -1 --oneline` shows `3ef2cf2` or a descendant on `main`.
- `grep -rn 'transition_to_queued\|peek_next_song' --include='*.rs' .` enumerate call sites (you should see roughly: 2 in `data/src/services/queue/navigation.rs` impls, 2 in `data/src/services/playback.rs`, 1 in `data/src/backend/playback_controller.rs`, plus tests). If hits in `src/` (the UI crate) appear, STOP — Lane B is supposed to be data/-only.

### 2. Add the typestate guard

In `data/src/services/queue/navigation.rs`, after the `PreviousSongResult` enum, add:

```rust
/// Borrow guard returned by [`QueueManager::peek_next_song`].
///
/// Owns the only public path to commit the peek to a transition.
/// Dropping without calling [`Self::transition`] runs `clear_queued()`
/// — the "abandon peek" path is a clean reset rather than silent
/// stale-queued state.
pub struct PeekedQueue<'a> {
    mgr: Option<&'a mut QueueManager>,
    info: NextSongResult,
}

impl<'a> PeekedQueue<'a> {
    pub fn song(&self) -> &Song { &self.info.song }
    pub fn index(&self) -> usize { self.info.index }
    pub fn reason(&self) -> &str { &self.info.reason }
    pub fn info(&self) -> &NextSongResult { &self.info }

    /// Consume the peek and advance current_index/current_order.
    pub fn transition(mut self) -> TransitionResult {
        let mgr = self.mgr.take().expect("guard already consumed");
        mgr.transition_to_queued_internal()
            .expect("PeekedQueue invariant: queued is set when guard exists")
    }
}

impl<'a> Drop for PeekedQueue<'a> {
    fn drop(&mut self) {
        if let Some(mgr) = self.mgr.take() {
            mgr.clear_queued();
        }
    }
}
```

Re-export `PeekedQueue` from `data/src/services/queue/mod.rs:14` alongside the other `pub use navigation::*` items.

### 3. Migrate `peek_next_song`

Change the existing `peek_next_song(&mut self) -> Option<NextSongResult>` (line 47) to return `Option<PeekedQueue<'_>>`. Internally:

- Compute the `NextSongResult` via the existing body.
- At the success return site, build `PeekedQueue { mgr: Some(self), info: <the result> }` and `Some(...)` it.
- At every `return None` and the trailing `None` cases, return `None` as before.

### 4. Rename `transition_to_queued`

- Rename the existing `pub fn transition_to_queued` (line 146) to `pub(crate) fn transition_to_queued_internal`. Body unchanged.
- Update the internal `get_next_song` (line 227) to call `transition_to_queued_internal`.
- Update the internal `peek_then_transition` test helpers in `navigation.rs` tests to use `transition_to_queued_internal`.

### 5. Migrate external callers

Two files in `data/`. The UI crate has no peek/transition callers.

**`data/src/services/playback.rs`**:

- Line 300: `if needs_load && queue_manager.peek_next_song().is_none() {` — leave as-is. `Option<PeekedQueue<'_>>` still has `.is_none()`. The peek's Drop runs immediately after this expression and clears queued (intended).
- Lines 321-324: replace
  ```rust
  if queue_manager.get_queue().queued.is_none() && !needs_load {
      debug!(" [TRACK FINISHED] queued was cleared (concurrent queue mutation), re-peeking");
      queue_manager.peek_next_song();
  }
  let Some(transition) = queue_manager.transition_to_queued() else {
      drop(queue_manager);
      debug!(" No queued song to transition to");
      return TrackTransitionPlan::Stop;
  };
  ```
  with
  ```rust
  let Some(peeked) = queue_manager.peek_next_song() else {
      drop(queue_manager);
      debug!(" No queued song to transition to");
      return TrackTransitionPlan::Stop;
  };
  let transition = peeked.transition();
  ```
  The fresh peek subsumes the re-peek defense and the transition.

- Line 515 inside `play_previous`: `queue_manager.insert_song_at(...)` is unchanged here — that's IG-2 / Lane A. Don't touch it.

**`data/src/backend/playback_controller.rs:521`**: gapless prep currently does
```rust
if let Some(ref next_result) = queue_manager.peek_next_song() {
    // ... uses next_result.song, next_result.index ...
}
```
Migrate to:
```rust
if let Some(peeked) = queue_manager.peek_next_song() {
    let next_song = peeked.song().clone();
    let next_index = peeked.index();
    drop(peeked); // explicit: clears queued; gapless prep proceeds with the captured data
    // ... use next_song / next_index ...
}
```
(Adjust the captured fields to whatever the existing body actually reads.)

### 6. Migrate tests

Both `data/src/services/queue/mod.rs` (lines ~858–940) and `data/src/services/queue/navigation.rs` (lines ~313–520, plus the proptest at ~1100). Pattern:

- `let peeked = qm.peek_next_song().unwrap();` then asserts on `peeked.index`/`peeked.song.id`/`peeked.reason` → change asserts to `peeked.index()`, `peeked.song().id`, `peeked.reason()`.
- `qm.transition_to_queued().unwrap()` followed by asserts → replace with `let peeked = qm.peek_next_song().unwrap(); let result = peeked.transition();` (transition returns `TransitionResult`, not Option).
- Side-effect-only peeks (`qm.peek_next_song();`) → keep, but note Drop now clears queued. If a test asserted `qm.queue.queued.is_some()` *after* a bare peek, the test must change — Drop ran. The expected new shape: hold the guard in scope while asserting (`let _peeked = qm.peek_next_song();`).
- The proptest `peek_is_idempotent` (line ~1100): two consecutive bare peeks now both clear queued on Drop. Idempotency still holds at the *return value* level — `peek1.map(|p| (p.index(), p.song().id.clone()))` should equal the same for `peek2`. Reshape the assert accordingly.

### 7. Commit slices

Recommended slice cadence (commit each verified slice without asking — this is a feature branch in a worktree per global feedback):

1. `refactor(queue): introduce PeekedQueue typestate guard` — guard struct + impls + the `peek_next_song` signature change. May leave external callers temporarily broken if you do this slice with `unimplemented!()` shims, but cleanest is to do everything in slice 1 if it stays under ~250 lines.
2. `refactor(queue): rename transition_to_queued to transition_to_queued_internal` — narrows the public surface.
3. `refactor(playback): migrate peek/transition flow to PeekedQueue guard` — `services/playback.rs` + `backend/playback_controller.rs` migrations.
4. `test(queue): migrate peek/transition tests to guard accessors` — both test modules + proptest.

Each slice must pass `cargo test && cargo clippy --all-targets -- -D warnings && cargo +nightly fmt --all -- --check`. Skip the `Co-Authored-By` trailer.

### 8. Update audit tracker

After the final commit, append commit refs to `.agent/audit-progress.md` §7 row 12 + §6 row IG-4. Do not mark §7 #12 fully done unless Lane A and Lane C have also landed.

## What NOT to touch

- Mutation methods other than the renamed `transition_to_queued_internal` (Lane C wraps them).
- `get_queue_mut`, `insert_song_at` (Lane A).
- The UI crate.
- `.agent/rules/` files (Lane C may update `gotchas.md` to reflect the new shape; that's not Lane B's job).

## If blocked

- If a peek call site exists in `src/` that the plan didn't anticipate: stop and ask.
- If a test depends on "peek leaves queued set": preserve that semantics by holding the guard alive in scope; if the test genuinely needs the OLD silent-stale-queued behavior, stop and ask before deleting it.
- If `cargo test` reveals a property failure in the proptest: investigate before adjusting the assert; the property should still hold at the result level.

## Reporting

End with: commits (refs + subjects), file diff line counts, and any test that needed a non-trivial reshape (one sentence each).
````

### lane-c-write-guard

worktree: ~/nokkvi-queue-igs-c
branch: refactor/queue-igs-write
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane C of the queue type-level invariants plan — close IG-5 (QueueWriteGuard with auto-clear-queued + named save modes), and align `set_repeat` / `toggle_consume` with `toggle_shuffle` while we're there.

Plan doc: /home/foogs/nokkvi/.agent/plans/queue-typestate-igs.md (section 2.2 for the design, section 4 "Lane C" for the file/method inventory).

Working directory: ~/nokkvi-queue-igs-c (this worktree). Branch: refactor/queue-igs-write. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `3ef2cf2` or a descendant on `main`.
- `grep -n 'clear_queued' data/src/services/queue/mod.rs` should show ~11 call sites in mutation methods (plus the `pub(crate) fn` definition in `order.rs`).
- `grep -rn '\.write()' data/src/services/queue/ --include='*.rs'` should return nothing — the new helper name `write` must not collide.

### 2. Add `QueueWriteGuard<'a>`

Either:
- a new submodule `data/src/services/queue/write_guard.rs` declared `mod write_guard;` in `mod.rs`, exporting `pub(crate) use write_guard::QueueWriteGuard;`, OR
- inline at the top of `data/src/services/queue/mod.rs` after the `KEY_QUEUE_*` constants.

Use the new-file approach if `mod.rs` would grow past ~1380 lines after the refactor; inline otherwise. Implementer's call.

Implement exactly per plan §2.2:

```rust
pub struct QueueWriteGuard<'a> {
    mgr: Option<&'a mut QueueManager>,
}

impl<'a> Drop for QueueWriteGuard<'a> {
    fn drop(&mut self) {
        if let Some(mgr) = self.mgr.take() {
            mgr.clear_queued();
        }
    }
}

impl<'a> QueueWriteGuard<'a> {
    pub fn commit_save_all(mut self) -> Result<()> {
        let mgr = self.mgr.take().expect("guard already consumed");
        mgr.clear_queued();
        mgr.save_all()
    }
    pub fn commit_save_order(mut self) -> Result<()> {
        let mgr = self.mgr.take().expect("guard already consumed");
        mgr.clear_queued();
        mgr.save_order()
    }
    pub fn commit_no_save(mut self) {
        let mgr = self.mgr.take().expect("guard already consumed");
        mgr.clear_queued();
    }
}

impl<'a> std::ops::Deref for QueueWriteGuard<'a> {
    type Target = QueueManager;
    fn deref(&self) -> &QueueManager {
        self.mgr.as_deref().expect("guard already consumed")
    }
}

impl<'a> std::ops::DerefMut for QueueWriteGuard<'a> {
    fn deref_mut(&mut self) -> &mut QueueManager {
        self.mgr.as_deref_mut().expect("guard already consumed")
    }
}

impl QueueManager {
    fn write(&mut self) -> QueueWriteGuard<'_> {
        QueueWriteGuard { mgr: Some(self) }
    }
}
```

### 3. Refactor every mutation method

For each method below, the body shape becomes:

```rust
pub fn <name>(&mut self, ...) -> Result<()> {
    // Optional pre-validation that returns Ok(())/Err early WITHOUT taking the guard.
    let mut tx = self.write();
    // ... body using `tx.<field>` / `tx.<helper>` via DerefMut ...
    tx.<commit_method>()
}
```

For methods that historically returned `Ok(())` after a no-op early check (e.g., `move_item`'s `from == to` branch), keep the early `return Ok(())` BEFORE `let mut tx = self.write();` so a no-op call doesn't fire `clear_queued`. The guard is only taken when work actually happens.

Methods (lines as of baseline `3ef2cf2`):

| Line | Method | Save mode | Notes |
|---:|---|---|---|
| 129 | `add_songs` | `commit_save_all` | |
| 147 | `set_queue` | `commit_save_all` | also clears `playback_history` — fine, do via `tx.playback_history.clear()` |
| 170 | `remove_song` | `commit_save_all` | conditional save: only when index in bounds. Pre-check before taking guard, OR `tx.commit_no_save()` on the out-of-range branch. |
| 231 | `toggle_shuffle` | `commit_save_order` | |
| 249 | `shuffle_queue` | `commit_save_order` | early-return on empty queue WITHOUT taking guard |
| 284 | `sort_queue` | `commit_save_order` | early-return on empty + delegation to `shuffle_queue` for `Random` — both before `self.write()` |
| **354** | **`set_repeat`** | **`commit_save_order`** | NEW: now goes through guard, now clears queued. Behavior alignment with `toggle_shuffle`. |
| **360** | **`toggle_consume`** | **`commit_save_order`** | NEW: same as set_repeat. |
| 411 | `set_current_index` | `commit_no_save` | does not return Result today (returns `()`). Keep that signature; commit_no_save returns `()`. |
| 424 | `move_item` | `commit_save_order` | early-return on `from >= len || to > len || from == to` BEFORE the guard |
| 467 | `insert_after_current` | `commit_save_all` | |
| 502 | `insert_song_and_make_current` (post-Lane A) | delegated body — leave as-is from Lane A | already routes through `insert_songs_at` and `set_current_index` |
| 521 | `insert_songs_at` | `commit_save_all` | early-return on `songs.is_empty()` BEFORE the guard |

After this refactor, every `self.clear_queued()` call inside `mod.rs` mutation methods is gone — the guard's Drop / commit handles it. Verify via `grep -n 'clear_queued' data/src/services/queue/mod.rs` — it should match only inside the WriteGuard impl (or zero if you placed the guard in `write_guard.rs`).

### 4. Tighten visibility

- After step 3, check whether `clear_queued` (currently `pub(crate)` in `data/src/services/queue/order.rs:76`) still has external callers:
  ```
  grep -rn '\.clear_queued()' data/src/services/queue/ --include='*.rs'
  ```
- If `navigation.rs` still calls it (lines 261, 283 in `get_previous_song`), keep `pub(crate)`. If it's used only by the guard's Drop and the navigation paths above, narrow to `pub(super)` if the visibility scopes line up.
- Do NOT remove the `clear_queued` method itself — both guards and `get_previous_song` rely on it.

### 5. New tests for set_repeat / toggle_consume

In `data/src/services/queue/mod.rs` test module, mirror the existing `queue_mutation_clears_queued` (line ~923) for the two newly-aligned methods:

```rust
#[test]
fn set_repeat_clears_queued() {
    let songs = vec![make_test_song("a"), make_test_song("b"), make_test_song("c")];
    let mut qm = make_test_manager(songs, Some(0));
    qm.peek_next_song(); // sets queued
    assert!(qm.queue.queued.is_some());
    qm.set_repeat(RepeatMode::Track).unwrap();
    assert!(qm.queue.queued.is_none(), "set_repeat must clear queued (IG-5)");
}

#[test]
fn toggle_consume_clears_queued() {
    let songs = vec![make_test_song("a"), make_test_song("b"), make_test_song("c")];
    let mut qm = make_test_manager(songs, Some(0));
    qm.peek_next_song();
    assert!(qm.queue.queued.is_some());
    qm.toggle_consume().unwrap();
    assert!(qm.queue.queued.is_none(), "toggle_consume must clear queued (IG-5)");
}
```

(If Lane B has merged ahead of you and `peek_next_song` returns `Option<PeekedQueue<'_>>`, the `qm.peek_next_song();` calls drop the guard immediately, which under Lane B's design ALSO clears queued — making these tests trivially pass even pre-refactor. To make the test meaningful, set `qm.queue.queued = Some(0)` directly via the test helper module's `pub(crate)` access if needed.)

### 6. Update gotchas/rules

After the refactor lands, the gotcha "every queue mutation must call `clear_queued()`" in `.agent/rules/gotchas.md` (if it exists there — verify) and in `~/nokkvi/CLAUDE.md` becomes obsolete: the type system enforces it. Update the relevant doc paragraph to:

> Queue mutations go through `QueueWriteGuard` (`qm.write()`); Drop runs `clear_queued()`. New mutator methods follow the `let mut tx = self.write(); ...; tx.commit_save_*()` shape.

Only edit docs that you can verify currently say something now-incorrect. Don't volunteer edits beyond that.

### 7. Commit slices

Commit each verified slice without pausing:

1. `refactor(queue): add QueueWriteGuard typestate with named commit modes` — guard struct + write() helper.
2. `refactor(queue): wrap add/set/remove mutators in QueueWriteGuard` — first three mutators (add_songs, set_queue, remove_song).
3. `refactor(queue): wrap shuffle/sort/move mutators in QueueWriteGuard` — toggle_shuffle, shuffle_queue, sort_queue, move_item.
4. `refactor(queue): wrap insert mutators in QueueWriteGuard` — insert_after_current, insert_songs_at, insert_song_and_make_current (verify still delegates correctly post-Lane A).
5. `refactor(queue): align mode-toggle mutators with QueueWriteGuard (set_repeat/toggle_consume)` — the behavior-alignment slice; include the two new tests; reference IG-5 in commit body.
6. `refactor(queue): wrap set_current_index in QueueWriteGuard` — separate slice because it's the no-save variant.
7. (optional) `refactor(queue): narrow clear_queued visibility` if step 4 of this brief justified it.
8. (optional) `docs: refresh queue invariant notes for type-level enforcement` — gotchas / CLAUDE.md.

Each slice: `cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo +nightly fmt --all -- --check`. Skip `Co-Authored-By` trailer.

### 8. Update audit tracker

Append commit refs to `.agent/audit-progress.md` §7 row 12 and §6 rows IG-3-queue-side / IG-5. Mark §7 #12 fully done ONLY if Lanes A and B have also merged; otherwise leave it 🟡 partial with a note about which lanes have landed.

## What NOT to touch

- `peek_next_song` / `transition_to_queued` (Lane B).
- `get_queue_mut`, `insert_song_at` (Lane A — pre-rename).
- The UI crate.
- The audio engine, renderer, or any IG-3 engine-side enforcement.
- Any other audit item.

## If blocked

- If a mutation method has a structure that doesn't fit `let mut tx = self.write(); ...; tx.commit_save_*()` — e.g., it currently mutates AND returns intermediate values that need to outlive the guard — restructure to compute those values from `tx.<field>` accessors before commit. If genuinely impossible, stop and ask.
- If `cargo test` regresses on an existing test that depends on `set_repeat` / `toggle_consume` NOT clearing queued: that's a real surprise — stop and report. The behavior alignment is intentional but a regression in a downstream test means we missed a contract.
- If a clippy warning fires on the guard impl (e.g., `must_use` recommendations on `commit_*`): apply the recommendation; don't `#[allow]`.

## Reporting

End with: commits (refs + subjects), `clear_queued` call-site count delta in `mod.rs` (should drop from ~11 to 0), and the new test additions.
````
