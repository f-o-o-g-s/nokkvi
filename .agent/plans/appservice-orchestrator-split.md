# AppService entity×verb split — fanout plan (§7 #7 / DRY #4 / Drift cross-cutting)

Closes `.agent/audit-progress.md` §7 #7. Today the 4 library entities × ~5 queue-action verbs are encoded as **~25 hand-written symmetric methods** on `AppService` (`play_album`, `play_artist`, `play_genre`, `play_playlist`, plus `add_*_to_queue`, `add_*_and_play`, `insert_*_at_position`, `play_next_*`, plus `_random` / `_from_track` variants). Adding a new entity (e.g. radio stations participating in queue ops) currently means hand-writing ~5 new symmetric methods — exactly the silent-omission shape the audit weights highest. This plan extracts a `SongSource` enum + two thin orchestrator types (`LibraryOrchestrator`, `QueueOrchestrator`), folds the 25 method bodies into 2-3 line delegations, and preserves every public AppService method name so UI handlers stay byte-identical.

Last verified baseline: **2026-05-09, `main @ HEAD = c45258b`** (`data/src/backend/app_service.rs` 970 LOC; 25 entity×verb / batch methods enumerated below; 22 UI call sites in `src/update/{albums,artists,genres,playlists,roulette}.rs`).

Source reports: `~/nokkvi-audit-results/{_SYNTHESIS,monoliths-data,dry-handlers,dry-api-calls,backend-boundary,drift-triangle,drift-match-arms,dry-tests}.md`. The audit's primary first-class evidence for this refactor is **`monoliths-data.md` §2** and **`backend-boundary.md` §2** — both quoted inline below.

---

## 1. Goal & rubric

The `AppService` entity×verb matrix is the **largest copy-paste amplifier in the data crate**. The audit (`monoliths-data.md` §2) frames it:

> "5 actions × 1 dispatch = 5 methods. Caller writes `app.play(SongSource::Album(id)).await?`."

Today every {Album, Artist, Genre, Playlist} × {play, add_to_queue, add_and_play, insert_at_position, play_next} cell is its own method body. The differences across cells are mechanical:
- which `*_service.load_*_songs` (or private `load_genre_songs` / `load_playlist_songs` helper) resolves the entity to `Vec<Song>`,
- which queue/playback primitive consumes the songs.

Per-cell quirks are minor and reducible to enum data:

| Variation axis | Album | Artist | Genre | Playlist |
|---|---|---|---|---|
| Resolve helper | `albums_service.load_album_songs` | `artists_service.load_artist_songs` | `load_genre_songs` (private, builds `SongsApiService` on demand) | `load_playlist_songs` (private, builds `PlaylistsApiService` on demand) |
| ID/name shape | `album_id: &str` | `artist_id: &str` | `genre_name: &str` (string name, not ID) | `playlist_id: &str` |
| `_random` variant | n/a | `play_artist_random` (random start_index) | `play_genre_random` (random start_index) | n/a |
| `_from_track` variant | `play_album_from_track(track_idx)` | n/a | n/a | `play_playlist_from_track(track_idx)` |
| `insert_*_at_position` | yes | yes | yes | **no** (audit notes the matrix isn't fully symmetric — playlist has no insert variant) |

Rubric (in order):

1. **Bug-class prevention.** Adding a new entity is one `SongSource` variant + one `resolve_*` arm. Adding a new verb is one `QueueOrchestrator` method. Per-entity quirks (genre's name-not-id, the random/from-track variants, playlist's missing insert cell) become typed parameters rather than body-divergence.
2. **Public API stable.** Every existing `AppService::play_album/play_artist/.../play_next_playlist` method keeps its name and signature. UI handlers in `src/update/{albums,artists,genres,playlists,roulette}.rs` (22 verified call sites) **do not change**. The audit (`backend-boundary.md` §2) is explicit on this: "UI handlers do not change — they continue calling through `AppService` (the shell). The internal refactor is transparent to the UI."
3. **Test signal preservation.** Every existing test passes unchanged. Coverage for the new orchestrator types is added via direct unit tests in their own modules.
4. **Foundation for §7 #5 (`ItemKind`) and §7 #8 (`LoaderTarget`).** A `SongSource` enum is the natural carrier for both follow-ups; this plan introduces it cleanly so those audits drop in additively.
5. **Genre's name-vs-id quirk stays visible.** `SongSource::Genre(String)` carries a name, not an ID, mirroring the Navidrome API contract. Documented in the enum doc-comment and in `LibraryOrchestrator::resolve_genre`.

---

## 2. Architecture

Three structures, all idiomatic Rust, no new dependencies. The orchestrators borrow existing `AppService` fields through short-lived `'a`-borrowed handles — no new fields on `AppService` (so the constructor signature stays unchanged), no `Arc` / mutex wrapping.

### 2.1 `SongSource` enum — closes the entity dimension

**Location**: new module `data/src/types/song_source.rs`, declared in `data/src/types/mod.rs`.

```rust
//! Source-of-songs descriptor for library → queue dispatch.
//!
//! Every queue verb (`play`, `enqueue`, `play_next`, `insert_at`) accepts a
//! `SongSource`, which `LibraryOrchestrator::resolve` turns into `Vec<Song>`.
//! Pre-resolved song lists (search results, batch-flattened multi-selections,
//! restored queue state) bypass resolution via the `Preloaded` variant.

use crate::types::song::Song;

#[derive(Debug, Clone)]
pub enum SongSource {
    /// Resolve via `albums_service.load_album_songs(album_id)`.
    Album(String),
    /// Resolve via `artists_service.load_artist_songs(artist_id)`.
    Artist(String),
    /// Resolve via on-demand `SongsApiService::load_songs_by_genre(genre_name)`.
    /// Note: genre is keyed by NAME, not ID, per Navidrome API.
    Genre(String),
    /// Resolve via on-demand `PlaylistsApiService::load_playlist_songs(playlist_id)`.
    Playlist(String),
    /// Already-resolved songs — skip the load step entirely.
    Preloaded(Vec<Song>),
}
```

The audit (`monoliths-data.md` §2 lines 374-378) considered three factor-out shapes — enum dispatch, trait + ZST impls, status-quo + macro — and **explicitly recommends the enum**:

> "(1) `enum SongSource { Album(id), Artist(id), Genre(id), Playlist(id), Preloaded(Vec<Song>) }` + a single `async fn load(&self, src: SongSource) -> Result<Vec<Song>>`. Then 5 actions × 1 dispatch = 5 methods. Caller writes `app.play(SongSource::Album(id)).await?`."

The trait+ZST shape (the same shape pending-expand uses for `ResolveSpec`) is rejected here because the data crate is iced-free, has no trait-object machinery, and the enum variants directly carry the entity ID/name without indirection.

**Why no `Random` / `FromTrack` sub-variants on the enum**: per the audit, the `_random` and `_from_track` quirks are **dispatch-time parameters on the queue verb**, not entity-side variations. `play_album_from_track(album_id, track_idx)` becomes `library.resolve(SongSource::Album(id))` followed by `queue.play(songs, track_idx)` — the offset is a queue-side concern. `play_artist_random` becomes resolve + compute random index + `queue.play(songs, random_idx)`.

### 2.2 `LibraryOrchestrator` — closes the resolve side

**Location**: new module `data/src/backend/library_orchestrator.rs`, declared in `data/src/backend/mod.rs`.

```rust
//! Resolves a `SongSource` into `Vec<Song>` by dispatching to the appropriate
//! domain service or on-demand API constructor.
//!
//! Borrowed from `AppService` via `app.library_orchestrator()` — does not own
//! any state, holds short-lived references to existing services.

use crate::backend::{AlbumsService, ArtistsService, AuthGateway};
use crate::services::api::{playlists::PlaylistsApiService, songs::SongsApiService};
use crate::types::{song::Song, song_source::SongSource};
use anyhow::Result;

pub struct LibraryOrchestrator<'a> {
    auth: &'a AuthGateway,
    albums: &'a AlbumsService,
    artists: &'a ArtistsService,
}

impl<'a> LibraryOrchestrator<'a> {
    pub(crate) fn new(
        auth: &'a AuthGateway,
        albums: &'a AlbumsService,
        artists: &'a ArtistsService,
    ) -> Self {
        Self { auth, albums, artists }
    }

    /// Single dispatch entry point. Variants delegate to per-entity helpers below.
    pub async fn resolve(&self, source: SongSource) -> Result<Vec<Song>> {
        match source {
            SongSource::Album(id) => self.resolve_album(&id).await,
            SongSource::Artist(id) => self.resolve_artist(&id).await,
            SongSource::Genre(name) => self.resolve_genre(&name).await,
            SongSource::Playlist(id) => self.resolve_playlist(&id).await,
            SongSource::Preloaded(songs) => Ok(songs),
        }
    }

    pub async fn resolve_album(&self, album_id: &str) -> Result<Vec<Song>> {
        self.albums.load_album_songs(album_id).await
    }

    pub async fn resolve_artist(&self, artist_id: &str) -> Result<Vec<Song>> {
        self.artists.load_artist_songs(artist_id).await
    }

    /// Genre is keyed by name (Navidrome API contract). Constructs
    /// `SongsApiService` on demand — mirrors today's private
    /// `AppService::load_genre_songs` (app_service.rs:724-728).
    pub async fn resolve_genre(&self, genre_name: &str) -> Result<Vec<Song>> {
        let songs_api = SongsApiService::new(self.auth.client_arc());
        songs_api.load_songs_by_genre(genre_name).await
    }

    /// Constructs `PlaylistsApiService` on demand — mirrors today's private
    /// `AppService::load_playlist_songs` (app_service.rs:739-745).
    pub async fn resolve_playlist(&self, playlist_id: &str) -> Result<Vec<Song>> {
        let playlists_api = PlaylistsApiService::new(self.auth.client_arc());
        playlists_api.load_playlist_songs(playlist_id).await
    }
}
```

**Why a borrow-handle, not a stored field**: borrowing avoids changing `AppService::new()` / `AppService::new_with_storage()` signatures (which the call site memo from `~/nokkvi-audit-results/_SYNTHESIS.md` notes are sensitive — see the redb cached-storage gotcha in CLAUDE.md). Keeps the constructor untouched; lifetimes stay simple because the orchestrator is constructed inside a single `await` chain.

**`auth.client_arc()` assumption**: the `AuthGateway` already exposes a way to get an `Arc<reqwest::Client>` for API construction (used today by `AlbumsService::artwork_client` and the `*_api()` factory methods on `AppService`). Lane A's first sub-agent dispatch confirms the exact accessor name before implementing.

### 2.3 `QueueOrchestrator` — closes the verb side

**Location**: new module `data/src/backend/queue_orchestrator.rs`, declared in `data/src/backend/mod.rs`.

```rust
//! Five queue verbs that consume `Vec<Song>` and dispatch to existing
//! `QueueService` / `PlaybackController` primitives.
//!
//! Borrowed from `AppService` via `app.queue_orchestrator()` — like
//! `LibraryOrchestrator`, holds only references.

use crate::backend::{PlaybackController, QueueService};
use crate::types::song::Song;
use anyhow::Result;

pub struct QueueOrchestrator<'a> {
    queue: &'a QueueService,
    playback: &'a PlaybackController,
}

impl<'a> QueueOrchestrator<'a> {
    pub(crate) fn new(queue: &'a QueueService, playback: &'a PlaybackController) -> Self {
        Self { queue, playback }
    }

    /// Replace queue with `songs`, set current to `start_index`, start playback.
    /// Mirrors today's `play_album` etc. — the universal "play this entity now" verb.
    pub async fn play(&self, songs: Vec<Song>, start_index: usize) -> Result<()> {
        self.playback.play_songs_from_index(songs, start_index).await
    }

    /// Append to queue without changing playback state.
    /// Mirrors today's `add_*_to_queue` family.
    pub async fn enqueue(&self, songs: Vec<Song>) -> Result<()> {
        self.queue.add_songs(songs).await
    }

    /// Append, then jump-play the first newly-appended song.
    /// Mirrors today's `add_*_and_play` family. Records the pre-append
    /// queue length to know which index the new songs land at.
    pub async fn enqueue_and_play(&self, songs: Vec<Song>) -> Result<()> {
        if songs.is_empty() {
            return Ok(());
        }
        let first_id = songs[0].id.clone();
        let queue_index = self.queue.get_songs().len();
        self.queue.add_songs(songs).await?;
        self.playback.play_song_from_queue(&first_id, queue_index).await
    }

    /// Insert at an explicit position.
    /// Mirrors today's `insert_*_at_position` family.
    pub async fn insert_at(&self, songs: Vec<Song>, position: usize) -> Result<()> {
        self.queue.insert_songs_at(position, songs).await
    }

    /// Insert immediately after the current song (single splice).
    /// Mirrors today's `play_next_*` family + the private
    /// `AppService::play_next_songs` helper at app_service.rs:759-777.
    /// Refresh-from-queue is the existing behavior — keep it.
    pub async fn play_next(&self, songs: Vec<Song>) -> Result<()> {
        let current_idx = self.queue.get_current_index().await;
        let target = current_idx.map_or(0, |i| i + 1);
        self.queue.insert_songs_at(target, songs).await?;
        self.queue.refresh_from_queue().await
    }
}
```

**Audit anchor** (`backend-boundary.md` §2 — paraphrased shape; final method names align with audit recommendation):
- `play` / `enqueue` / `enqueue_and_play` / `insert_at` / `play_next` are the five canonical verbs. The audit also lists `remove_by_ids` for symmetry, but `remove_queue_songs` (app_service.rs:948-969) doesn't fit the entity×verb grid (it consumes IDs, not a `SongSource`) — keep it on `AppService` as today, do not migrate to the orchestrator.

**Existing helper resolution** (verified in agent-mapping pass against current code):
- `PlaybackController::play_songs_from_index(songs, start_index) -> Result<()>` — playback_controller.rs:596-640.
- `QueueService::add_songs(songs) -> Result<()>` — queue.rs:147-160.
- `QueueService::insert_songs_at(index, songs) -> Result<()>` — queue.rs:284-299.
- `PlaybackController::play_song_from_queue(song_id, queue_index) -> Result<()>` — playback_controller.rs:648-697.
- `QueueService::get_current_index` / `get_songs` / `refresh_from_queue` — exist in `queue.rs`. **Verify exact signatures during Lane B's sub-agent pass before implementing.** The plan's signatures above are the expected shape; if any drifted, Lane B reports back before writing.

### 2.4 `AppService` delegation pattern — closes the matrix

Every existing `AppService::play_album/play_artist/.../play_next_playlist` method is rewritten as a 2-3 line delegation through the two orchestrators. **All public method names + signatures preserved.** Bodies become:

```rust
pub async fn play_album(&self, album_id: &str) -> Result<()> {
    let songs = self.library_orchestrator().resolve_album(album_id).await?;
    self.queue_orchestrator().play(songs, 0).await
}

pub async fn play_album_from_track(&self, album_id: &str, track_idx: usize) -> Result<()> {
    let songs = self.library_orchestrator().resolve_album(album_id).await?;
    let start = track_idx.min(songs.len().saturating_sub(1));
    self.queue_orchestrator().play(songs, start).await
}

pub async fn play_genre_random(&self, genre_name: &str) -> Result<()> {
    let songs = self.library_orchestrator().resolve_genre(genre_name).await?;
    if songs.is_empty() {
        return Ok(());
    }
    let start = rand::random_range(0..songs.len());
    self.queue_orchestrator().play(songs, start).await
}

pub async fn add_album_to_queue(&self, album_id: &str) -> Result<()> {
    let songs = self.library_orchestrator().resolve_album(album_id).await?;
    self.queue_orchestrator().enqueue(songs).await
}

pub async fn add_album_and_play(&self, album_id: &str) -> Result<()> {
    let songs = self.library_orchestrator().resolve_album(album_id).await?;
    self.queue_orchestrator().enqueue_and_play(songs).await
}

pub async fn insert_album_at_position(&self, album_id: &str, position: usize) -> Result<()> {
    let songs = self.library_orchestrator().resolve_album(album_id).await?;
    self.queue_orchestrator().insert_at(songs, position).await
}

pub async fn play_next_album(&self, album_id: &str) -> Result<()> {
    let songs = self.library_orchestrator().resolve_album(album_id).await?;
    self.queue_orchestrator().play_next(songs).await
}
```

Three accessors are added to `AppService` (one new public method, two private helpers):

```rust
impl AppService {
    /// Borrow-handle for entity-to-songs resolution. Holds no state.
    pub(crate) fn library_orchestrator(&self) -> LibraryOrchestrator<'_> {
        LibraryOrchestrator::new(&self.auth_gateway, &self.albums_service, &self.artists_service)
    }

    /// Borrow-handle for queue mutation verbs. Holds no state.
    pub(crate) fn queue_orchestrator(&self) -> QueueOrchestrator<'_> {
        QueueOrchestrator::new(&self.queue_service, &self.playback)
    }
}
```

Three private helpers on `AppService` are **deleted** because their logic moves into the orchestrators:
- `load_genre_songs` (line 724) → `LibraryOrchestrator::resolve_genre`.
- `load_playlist_songs` (line 739) → `LibraryOrchestrator::resolve_playlist`.
- `play_next_songs` (line 759) → `QueueOrchestrator::play_next`.

**Song-keyed methods stay on AppService** as today (out of scope for this refactor — they don't fit the entity×verb pattern):
- `play_songs(songs, start_index)` (line 350) → trivially delegates: `self.queue_orchestrator().play(songs, start_index).await`.
- `add_song_to_queue(song)` (line 390) → `self.queue_orchestrator().enqueue(vec![song]).await`.
- `add_song_and_play(song)` (line 399) → `self.queue_orchestrator().enqueue_and_play(vec![song]).await`.
- `add_song_to_queue_by_id(song_id, album_id)` (line 411) — resolves album, finds song by ID, then enqueues the single song. **Keep current body**, it doesn't fit the orchestrator grid cleanly (the find-by-id step is bespoke).
- `insert_song_at_position(song, position)` (line 561) → `self.queue_orchestrator().insert_at(vec![song], position).await`.
- `insert_song_by_id_at_position` (line 574) — same as `add_song_to_queue_by_id` rationale; keep current body.
- `play_next_song_by_id` (line 786) — same.
- `play_next_preloaded` (line 816) → `self.queue_orchestrator().play_next(songs).await`.

**Batch methods stay on AppService** as today (out of scope; they consume `BatchPayload` via `resolve_batch`, not `SongSource`):
- `resolve_batch` (line 828), `play_batch` (line 872), `add_batch_to_queue` (line 889), `play_next_batch` (line 897), `insert_batch_at_position` (line 903), `remove_batch_from_queue` (line 916).

A future follow-up could extend `SongSource` with a `Batch(BatchPayload)` variant and migrate these too — that's a separate plan.

**`load_playlist_into_queue`** (line 336) — loads a playlist for edit-mode (no playback). Body becomes: `let songs = self.library_orchestrator().resolve_playlist(playlist_id).await?; self.queue_service.set_queue_for_edit(songs).await` (or whatever the existing post-load step is — preserve verbatim). Delegate the resolve, leave the queue-side untouched.

---

## 3. Lane decomposition (parallel)

Five lanes. Lanes A and B are pure additions in `data/src/` and run in true parallel. Lanes C and D each migrate a disjoint block of `app_service.rs` and rebase on Lanes A+B's branches before pushing. Lane E is independent (UI verification + audit tracker + test mirror check) and runs concurrently with everything else.

| Lane | Scope | Files touched | Commit count (est.) | Effort | Depends on |
|---|---|---|---:|---|---|
| **A** (LibraryOrchestrator + SongSource) | Pure additive types in `data/src/` | `data/src/types/song_source.rs` (new), `data/src/types/mod.rs`, `data/src/backend/library_orchestrator.rs` (new), `data/src/backend/mod.rs`, `data/src/backend/app_service.rs` (accessor only, ~5 LOC near line 250) | 3-4 | M | none |
| **B** (QueueOrchestrator) | Pure additive types in `data/src/` | `data/src/backend/queue_orchestrator.rs` (new), `data/src/backend/mod.rs`, `data/src/backend/app_service.rs` (accessor only, ~5 LOC near line 250) | 3-4 | M | none |
| **C** (play_* family migration) | 9 method bodies become 2-3 line delegations | `data/src/backend/app_service.rs` (lines 258-358 only) | 4-5 | M | A + B |
| **D** (mutation families migration + private-helper deletion) | 16 entity×verb method bodies + delete 3 private helpers + delegate the song-keyed and batch-adjacent methods that fit | `data/src/backend/app_service.rs` (lines 336-820 across the add/insert/play_next blocks) | 5-7 | L | A + B |
| **E** (UI verify + test mirror + audit tracker) | UI call-site smoke verification, test-mirror-search via sub-agent, `.agent/audit-progress.md` flip | `.agent/audit-progress.md`, possibly `src/update/tests/` (only if mirror found) | 1-3 | S | A + B + C + D |

**Conflict zones**:

- **Lane A and Lane B both edit `data/src/backend/mod.rs`**: each adds one `mod *_orchestrator;` line. Trivial textual conflict; whichever lands first, the other rebases by adding one line. Same for `data/src/backend/app_service.rs` — both lanes add an accessor (`library_orchestrator()` for A, `queue_orchestrator()` for B). Place each in its own `impl AppService` block (Lane A under a `// === LibraryOrchestrator accessor ===` banner, Lane B under `// === QueueOrchestrator accessor ===`) so the diffs sit at different line numbers and rebase mechanically.
- **Lane C and Lane D both edit `data/src/backend/app_service.rs`** at disjoint line ranges. Lane C owns lines 258-358 (`play_*` family). Lane D owns lines 336 (`load_playlist_into_queue`), 368-650 (add/insert), 759 (private `play_next_songs`), 780-822 (`play_next_*` family), and the song-keyed delegations at 350, 390, 399, 561, 816. Since file shrinks ~250 LOC during these migrations, the line numbers shift mid-merge — but the methods themselves are stable named entities (`grep -n 'fn play_album' app_service.rs` finds them regardless of line shift). Whichever of C/D lands first, the other rebases by re-anchoring on method name.
- **Recommended merge order: A → B → C → D → E**. A↔B can swap. C↔D can swap.
- **Lane E** depends on the others *for the audit tracker flip* but its sub-agent dispatches (test-mirror search, UI smoke verification) can run as soon as the lane spawns — they read existing code and report findings, not block-on-merge.

---

## 4. Per-lane scope (callers verified at baseline `c45258b`)

### Lane A — `LibraryOrchestrator` + `SongSource`

**Files**:
- `data/src/types/song_source.rs` — new module with the enum.
- `data/src/types/mod.rs` — add `pub mod song_source;` and `pub use song_source::SongSource;`.
- `data/src/backend/library_orchestrator.rs` — new module with the orchestrator.
- `data/src/backend/mod.rs` — add `pub mod library_orchestrator;` and `pub use library_orchestrator::LibraryOrchestrator;`.
- `data/src/backend/app_service.rs` — add `pub(crate) fn library_orchestrator(&self) -> LibraryOrchestrator<'_>` accessor (no other body changes). Place it in a fresh `impl AppService` block under a banner comment, near the existing accessor methods (search for `genres_api(&self)` / `playlists_api(&self)` — those are the existing peer accessors, around lines 230-250).

**Verification before writing**:
- The `auth_gateway: AuthGateway` field exists on `AppService` (verified — line ~30).
- `AuthGateway` exposes some accessor that hands out a `reqwest::Client` (or `Arc<reqwest::Client>`) for API construction. **Lane A's first sub-agent confirms the exact name/shape** before writing the orchestrator (could be `client()`, `client_arc()`, `subsonic_client()`, etc.). If no such accessor exists, Lane A adds the smallest one needed (a `pub fn client_arc(&self) -> Arc<reqwest::Client>` on `AuthGateway`) — note this in the commit body.

**Tests** (in `data/src/backend/library_orchestrator.rs` `#[cfg(test)]` module):
- `resolve_dispatches_album_variant_to_albums_service` — happy-path Album.
- `resolve_dispatches_artist_variant_to_artists_service` — happy-path Artist.
- `resolve_preloaded_returns_input_unchanged` — no-load fast path.
- `resolve_genre_constructs_songs_api_with_correct_name` — exercises the on-demand-API construction path (mock or stub the API; assertion is on the genre name passed through).
- `resolve_playlist_constructs_playlists_api_with_correct_id` — same shape.

If mocking is too heavy for the genre/playlist on-demand tests, those tests may stay as compile-only smoke (instantiate the orchestrator, call `resolve` with a mocked services struct, assert no panic). Implementer's call.

### Lane B — `QueueOrchestrator`

**Files**:
- `data/src/backend/queue_orchestrator.rs` — new module.
- `data/src/backend/mod.rs` — add `pub mod queue_orchestrator;` and `pub use queue_orchestrator::QueueOrchestrator;`.
- `data/src/backend/app_service.rs` — add `pub(crate) fn queue_orchestrator(&self) -> QueueOrchestrator<'_>` accessor in its own `impl` block under a banner.

**Verification before writing**:
- `QueueService::get_current_index` — exists and returns `Option<usize>` (or similar). **Lane B's first sub-agent confirms the exact signature** before writing `play_next`.
- `QueueService::get_songs` — exists and returns a slice/vec of current songs.
- `QueueService::refresh_from_queue` — exists. The plan assumes `play_next` calls it post-insert to mirror the current `AppService::play_next_songs` body at line 759-777. If the existing helper is shaped differently (e.g., refresh is internal to `insert_songs_at`), Lane B reports back before implementing.

**Tests** (in `data/src/backend/queue_orchestrator.rs` `#[cfg(test)]`):
- `play_replaces_queue_and_starts_at_index` — happy-path play.
- `enqueue_appends_without_changing_current` — mode invariance.
- `enqueue_and_play_jumps_to_first_appended` — pre-append index capture.
- `enqueue_and_play_noop_on_empty_input` — early-return guard.
- `insert_at_passes_position_through` — argument fidelity.
- `play_next_inserts_at_current_plus_one` — splice arithmetic.
- `play_next_when_no_current_inserts_at_zero` — empty-queue edge.

### Lane C — `play_*` family migration

**Files**:
- `data/src/backend/app_service.rs` only — lines 258-358 region. Method-by-method bodies replaced. **No other file touched.**

**Sites to migrate** (all 9 methods become 2-3 line delegations):

| Line | Method | New body (sketch) |
|---:|---|---|
| 258 | `play_album` | `let s = self.library_orchestrator().resolve_album(album_id).await?; self.queue_orchestrator().play(s, 0).await` |
| 266 | `play_album_from_track` | resolve_album + clamp `track_idx.min(s.len().saturating_sub(1))` + `play(s, start)` |
| 274 | `play_artist` | resolve_artist + `play(s, 0)` |
| 282 | `play_genre` | resolve_genre + `play(s, 0)` |
| 290 | `play_genre_random` | resolve_genre + early-return if empty + `rand::random_range` + `play(s, idx)` |
| 301 | `play_artist_random` | resolve_artist + early-return if empty + `rand::random_range` + `play(s, idx)` |
| 313 | `play_playlist` | resolve_playlist + `play(s, 0)` |
| 321 | `play_playlist_from_track` | resolve_playlist + clamp + `play(s, start)` |
| 350 | `play_songs(songs, start_index)` | direct: `self.queue_orchestrator().play(songs, start_index).await` (no resolve step — songs already in hand) |

**Critical note on `play_songs`**: today's body (lines 350-358) is already a thin delegation but stays for now. After migration, it's a 1-line direct call to `queue_orchestrator().play`. **Public signature unchanged.**

**Quirks to preserve**:
- `play_album_from_track` already clamps `track_idx` against `songs.len() - 1` inside `play_songs_from_index`; the migration preserves that clamp visibility by computing it explicitly in the new body (matching the pattern shown in §2.4 above). Don't re-clamp inside the orchestrator — keep it caller-side so the intent is visible at the AppService delegation.
- `play_*_random` uses `rand::random_range(0..songs.len())` after an empty-check. Preserve verbatim.
- All bodies preserve their existing `Result<()>` signature with `?` propagation. No silent error swallowing.

**External callers preserved** (verified via `rg "shell\.(play_album|play_album_from_track|play_artist|play_genre|play_playlist|play_songs|play_artist_random|play_genre_random)" src/`):
- `src/update/albums.rs:625, 782` — `play_album`, `play_album_from_track`.
- `src/update/artists.rs:467, 508` — `play_artist`, `play_album` (cross-call from artist view).
- `src/update/genres.rs:294, 337` — `play_genre`, `play_album` (cross-call from genre view).
- `src/update/playlists.rs:208` — `play_playlist`.
- `src/update/roulette.rs:356, 368` — `play_genre_random`, `play_artist_random`.
- `src/update/components.rs:508` — comment-only reference (in a docstring example), no actual call.

All 8 active call sites continue working without modification.

### Lane D — mutation families migration + private-helper deletion

**Files**:
- `data/src/backend/app_service.rs` only. Lines 336, 368-650, 724-777, 780-822.

**Sites to migrate** (16 entity×verb methods + 4 song-keyed delegations + 3 private-helper deletions):

**add_*_to_queue family (lines 368-446)**:

| Line | Method | New body |
|---:|---|---|
| 368 | `add_album_to_queue` | resolve_album + `enqueue` |
| 379 | `add_artist_to_queue` | resolve_artist + `enqueue` |
| 390 | `add_song_to_queue` | direct: `enqueue(vec![song])` |
| 425 | `add_genre_to_queue` | resolve_genre + `enqueue` |
| 436 | `add_playlist_to_queue` | resolve_playlist + `enqueue` |

**add_*_and_play family (lines 399-523)**:

| Line | Method | New body |
|---:|---|---|
| 399 | `add_song_and_play` | direct: `enqueue_and_play(vec![song])` |
| 411 | `add_song_to_queue_by_id` | **keep current body** (find-by-id is bespoke) |
| 453 | `add_album_and_play` | resolve_album + `enqueue_and_play` |
| 469 | `add_artist_and_play` | resolve_artist + `enqueue_and_play` |
| 488 | `add_genre_and_play` | resolve_genre + `enqueue_and_play` |
| 507 | `add_playlist_and_play` | resolve_playlist + `enqueue_and_play` |

**insert_*_at_position family (lines 533-609)**:

| Line | Method | New body |
|---:|---|---|
| 533 | `insert_album_at_position` | resolve_album + `insert_at(songs, position)` |
| 547 | `insert_artist_at_position` | resolve_artist + `insert_at(songs, position)` |
| 561 | `insert_song_at_position` | direct: `insert_at(vec![song], position)` |
| 574 | `insert_song_by_id_at_position` | **keep current body** (find-by-id is bespoke) |
| 598 | `insert_genre_at_position` | resolve_genre + `insert_at(songs, position)` |

(No `insert_playlist_at_position` — the matrix isn't fully symmetric, per the audit. Don't add one as part of this plan.)

**play_next_* family (lines 780-822)**:

| Line | Method | New body |
|---:|---|---|
| 780 | `play_next_album` | resolve_album + `play_next` |
| 786 | `play_next_song_by_id` | **keep current body** (find-by-id is bespoke) |
| 798 | `play_next_artist` | resolve_artist + `play_next` |
| 804 | `play_next_genre` | resolve_genre + `play_next` |
| 810 | `play_next_playlist` | resolve_playlist + `play_next` |
| 816 | `play_next_preloaded` | direct: `play_next(songs)` |

**Private helpers — DELETE**:
- `load_genre_songs` (lines 724-728) — moves into `LibraryOrchestrator::resolve_genre`.
- `load_playlist_songs` (lines 739-745) — moves into `LibraryOrchestrator::resolve_playlist`.
- `play_next_songs` (lines 759-777) — moves into `QueueOrchestrator::play_next`.

After deletion, every call site of these helpers (now only inside the entity×verb method bodies) routes through the orchestrator. **Verify zero remaining callers** before deleting: `rg 'load_genre_songs|load_playlist_songs|play_next_songs' data/src/backend/app_service.rs` should report only the function definitions themselves; if any non-definition site remains, migrate it first.

**`load_playlist_into_queue` (line 336)** — **keep current body shape**, but replace the load step with `self.library_orchestrator().resolve_playlist(playlist_id).await?`. The post-load `set_queue_for_edit` (or whatever it's called — verify the exact existing call) stays untouched.

**External callers preserved** (verified via the agent #2 mapping pass + this Lane's own rg sweep before committing):
- `src/update/albums.rs:591, 601, 614` — `insert_album_at_position`, `add_album_to_queue`, `add_album_and_play`.
- `src/update/artists.rs:433, 443, 456, 502` — `insert_artist_at_position`, `add_artist_to_queue`, `add_artist_and_play`, `add_album_to_queue` (cross-call).
- `src/update/genres.rs:268, 276, 287, 331` — `insert_genre_at_position`, `add_genre_to_queue`, `add_genre_and_play`, `add_album_to_queue` (cross-call).
- `src/update/playlists.rs:173, 189` — `add_playlist_to_queue`, `add_playlist_and_play`.

13 call sites total across these handlers; all continue working without modification.

### Lane E — UI verify + test-mirror search + audit tracker flip

**Files**:
- `.agent/audit-progress.md` — flip §7 #7 from `❌ open` to `✅ done` with commit refs from Lanes A-D, and update the §3 #4 row (which references §7 #7) to match.
- Possibly `src/update/tests/*.rs` — only if the sub-agent's test-mirror search finds an existing entity×verb tri/quad-mirror that the audit didn't catch.

**Sub-agent dispatch** (the lane-claude spawns these on entry):

1. **Test-mirror search (Explore agent)**: "Grep `src/update/tests/` for any test set that mirrors `play_album` / `play_artist` / `play_genre` / `play_playlist` (or the `add_*` / `insert_*` / `play_next_*` siblings) as parallel test bodies. Report file:line + test names. The audit's `dry-tests.md` already lists `tests/navigation.rs` as the primary mirror site (covered separately by `pending-expand-dedup.md`); I'm looking for OTHER mirrors that this AppService refactor exposes." If a mirror is found, the lane-claude proposes a fold (likely a `for_each_library_entity!` macro analogous to the pending-expand pattern); if no mirror is found, the lane-claude moves on to UI verification.

2. **UI smoke verification (general-purpose agent)**: "After Lanes A-D have landed on `main`, walk every UI call site listed below and confirm each still compiles, the `await` chain shape is unchanged, and the toast/error-routing semantics match. Sites: [paste the 22 verified call sites from §4 Lane C + Lane D]. Report any that look subtly different post-refactor — particularly any place where `?` propagation behavior changed because the orchestrator returns a different `Result<E>` type than the old method body."

3. **Audit tracker producer (general-purpose agent)**: "Read `.agent/audit-progress.md` §7 row 7 and §3 row 4. Produce the exact diff to:
   - flip §7 #7 status to `✅ done` with the format used by other ✅ rows (commit refs in monospace, lane breakdown sentence, evidence-line listing the deleted helpers + the new orchestrator types),
   - update §3 #4 to mirror,
   - add a one-line note to the 'Quick-pick' section if §7 #5 (`ItemKind`) or §7 #8 (`LoaderTarget`) becomes more accessible now that `SongSource` exists.
   Do not commit — return the diff for the lane-claude to apply manually after Lanes A-D commit refs are known."

---

## 5. Verification (every lane)

Run after each commit slice:

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass before pushing the slice. Per-lane TDD is light — Lanes A and B add new types with their own tests; Lanes C and D are structural (no behavior changes; every existing test must still pass).

**Lane A specific checks**:
- `cargo test -p nokkvi-data library_orchestrator::` lists the 5+ new tests.
- `wc -l data/src/backend/library_orchestrator.rs` reports ~200-280 LOC (orchestrator + tests).
- `grep -c 'mod library_orchestrator' data/src/backend/mod.rs` returns `1`.
- `grep -c 'fn library_orchestrator' data/src/backend/app_service.rs` returns `1`.

**Lane B specific checks**:
- `cargo test -p nokkvi-data queue_orchestrator::` lists the 7+ new tests.
- `wc -l data/src/backend/queue_orchestrator.rs` reports ~180-260 LOC.
- `grep -c 'mod queue_orchestrator' data/src/backend/mod.rs` returns `1`.
- `grep -c 'fn queue_orchestrator' data/src/backend/app_service.rs` returns `1`.

**Lane C specific checks**:
- `wc -l data/src/backend/app_service.rs` delta: ~50-80 LOC reduction from the play family alone.
- `grep -c 'fn play_album\|fn play_album_from_track\|fn play_artist\|fn play_artist_random\|fn play_genre\|fn play_genre_random\|fn play_playlist\|fn play_playlist_from_track\|fn play_songs' data/src/backend/app_service.rs` returns `9` (all method names preserved).
- Each migrated method body is ≤6 lines (search: `awk '/pub async fn play_/,/^    }$/' data/src/backend/app_service.rs | wc -l` should drop substantially).

**Lane D specific checks**:
- `wc -l data/src/backend/app_service.rs` delta: ~150-200 LOC reduction from the mutation families.
- `grep -c 'fn load_genre_songs\|fn load_playlist_songs\|fn play_next_songs' data/src/backend/app_service.rs` returns `0` (all three private helpers deleted).
- The 22 UI call sites still compile + tests pass.

**Lane E specific checks**:
- `.agent/audit-progress.md` §7 #7 row shows `✅ done` with commit refs in the format of other done rows.
- If a test mirror was found and folded: `cargo test` test count delta is `0` (every test name preserved or renamed to match).

---

## 6. What each lane does NOT do

- **No public AppService API rename or removal.** Every `play_album` / `add_artist_to_queue` / `play_next_genre` / etc. method keeps its name and signature. UI handlers in `src/update/*` stay byte-identical.
- **No new dependency.** Plan uses existing `anyhow::Result`, `rand::random_range`, and current `*Service` / `PlaybackController` / `QueueService` primitives.
- **No behavior change.** Toast text, error propagation, queue-mutation order, scrobble timing, gapless-prep reset semantics — all preserved.
- **No batch-method migration.** `resolve_batch`, `play_batch`, `add_batch_to_queue`, `play_next_batch`, `insert_batch_at_position`, `remove_batch_from_queue` (lines 828-946) stay as today. A future plan can extend `SongSource` with a `Batch(BatchPayload)` variant.
- **No song-by-id method migration.** `add_song_to_queue_by_id`, `insert_song_by_id_at_position`, `play_next_song_by_id` stay as today (find-by-id step is bespoke; doesn't fit the orchestrator grid cleanly).
- **No `remove_queue_songs` migration.** Consumes IDs not a `SongSource`; leave on `AppService`.
- **No new fields on `AppService`.** The orchestrators are borrow-handles, not stored fields. Constructor signatures (`AppService::new`, `AppService::new_with_storage`) stay identical — important because of the cached-storage relogin gotcha (CLAUDE.md "Database lock on re-login").
- **No expansion to a 5th entity** (e.g. radio stations, podcasts). Plan structurally enables it, but actually adding one is a follow-up.
- **No drive-by reformatting** outside touched files.
- **No update to `.agent/rules/`** files in Lanes A-D. Lane E may touch `.agent/audit-progress.md`. Rules-doc syncing is `/sync-rules`'s job.
- **No CI grep-test added.** `_SYNTHESIS.md §8` suggests a CI lint preventing future entity×verb fanout; that's a separate plan.
- **No collapse of `play_songs` / `add_song_to_queue` / `insert_song_at_position` / `play_next_preloaded` into the orchestrator.** They stay as 1-line delegations on `AppService` because UI sites pre-resolve songs and shouldn't be forced through `SongSource::Preloaded`. Keep the surface ergonomic.

---

## Fanout Prompts

### lane-a-library

worktree: ~/nokkvi-appservice-a
branch: refactor/appservice-library-orchestrator
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane A of the AppService entity×verb split — add `SongSource` enum + `LibraryOrchestrator` borrow-handle. Pure additive — does NOT migrate any existing AppService method bodies.

Plan doc: /home/foogs/nokkvi/.agent/plans/appservice-orchestrator-split.md (sections 2.1, 2.2, 4 "Lane A").

Working directory: ~/nokkvi-appservice-a (this worktree). Branch: refactor/appservice-library-orchestrator. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` should show `c45258b` or a descendant on `main`.
- `wc -l data/src/backend/app_service.rs` should report 970.
- `ls data/src/types/song_source.rs data/src/backend/library_orchestrator.rs 2>&1` should both report "No such file" — these are new.
- `rg -n 'pub mod song_source' data/src/types/mod.rs` should return nothing.
- `rg -n 'pub mod library_orchestrator' data/src/backend/mod.rs` should return nothing.

### 2. Sub-agent dispatch — confirm AuthGateway client accessor

Before writing the orchestrator, dispatch one Explore sub-agent:

```
Agent({
  description: "Find AuthGateway client accessor",
  subagent_type: "Explore",
  prompt: "Read /home/foogs/nokkvi/data/src/backend/auth.rs in full. Identify how other code currently obtains an `Arc<reqwest::Client>` (or `reqwest::Client`) from `AuthGateway` for constructing on-demand API services like `SongsApiService` and `PlaylistsApiService`. Look at how `AppService::genres_api()` / `playlists_api()` / `songs_api()` / `radios_api()` / `similar_api()` accessors do this today (they're somewhere around lines 200-260 in /home/foogs/nokkvi/data/src/backend/app_service.rs). Report: (1) the exact AuthGateway accessor name and signature, (2) the exact construction pattern used in app_service.rs:*_api() factories, (3) any imports needed. Under 300 words."
})
```

If the agent finds no public client accessor on `AuthGateway`, you have two options:
a. Add the smallest possible accessor (e.g. `pub fn client_arc(&self) -> Arc<reqwest::Client>`) on `AuthGateway` itself — note this in the commit.
b. Have `LibraryOrchestrator::new` take the constructed `*ApiService` instances as parameters instead of constructing them — orchestrator becomes a pure dispatcher. **(b) is cleaner if the existing `*_api()` factories on `AppService` already encapsulate construction; have the accessor wire them in.**

Pick whichever shape matches existing idiom. Do not invent a new pattern.

### 3. Write `data/src/types/song_source.rs`

Per plan §2.1. Five-variant enum:
- `Album(String)` — album ID
- `Artist(String)` — artist ID
- `Genre(String)` — genre NAME (per Navidrome API; document this in the variant doc-comment)
- `Playlist(String)` — playlist ID
- `Preloaded(Vec<Song>)` — already-resolved

`#[derive(Debug, Clone)]`. Module-level doc explaining the entity → resolution mapping.

### 4. Wire SongSource into types/mod.rs

Add at the appropriate alphabetical position:
```rust
pub mod song_source;
pub use song_source::SongSource;
```

### 5. Write `data/src/backend/library_orchestrator.rs`

Per plan §2.2. Structure:
- `pub struct LibraryOrchestrator<'a> { auth, albums, artists }` (or whatever the sub-agent's findings dictate for the field shape).
- `pub(crate) fn new(...)` constructor.
- `pub async fn resolve(&self, source: SongSource) -> Result<Vec<Song>>` dispatch method.
- `pub async fn resolve_album/resolve_artist/resolve_genre/resolve_playlist` per-variant methods. Genre and playlist construct the on-demand API services per the sub-agent's findings.

The genre and playlist resolvers replace the **logic** of today's private `AppService::load_genre_songs` (line 724) and `load_playlist_songs` (line 739) — but **DO NOT delete those AppService methods in this lane**. They stay; Lane D deletes them after migrating their callers.

Use `anyhow::Result`. Use `tracing::trace!` if you want a log line on each resolve, but match the verbosity of existing similar paths (don't suddenly add `info!` where today there's none).

### 6. Wire LibraryOrchestrator into backend/mod.rs

Add at the appropriate position:
```rust
pub mod library_orchestrator;
pub use library_orchestrator::LibraryOrchestrator;
```

### 7. Add accessor to AppService

In `data/src/backend/app_service.rs`, find where the existing `genres_api(&self)` / `playlists_api(&self)` / similar peer accessors live (around lines 230-250 — confirm via grep). Add a new `impl AppService` block near them, under a banner comment:

```rust
// === Library orchestrator accessor ===
impl AppService {
    /// Borrow-handle for entity-to-songs resolution. Holds no state; constructs on demand.
    pub(crate) fn library_orchestrator(&self) -> LibraryOrchestrator<'_> {
        LibraryOrchestrator::new(/* fields per the sub-agent's findings */)
    }
}
```

**Place the new `impl` block in its own location** (not folded into the main `impl AppService`) so Lane B's parallel `queue_orchestrator()` accessor lands without textual conflict.

### 8. Tests

Add `#[cfg(test)] mod tests` inside `library_orchestrator.rs`. Cover:
- `resolve_dispatches_album_variant_to_albums_service`
- `resolve_dispatches_artist_variant_to_artists_service`
- `resolve_preloaded_returns_input_unchanged`
- `resolve_genre_constructs_songs_api_with_correct_name` (compile-only smoke if mocking is too heavy)
- `resolve_playlist_constructs_playlists_api_with_correct_id` (same)

Use whatever existing test infrastructure is in `data/src/`. Look at `data/src/services/queue/*.rs` tests for the in-crate test pattern. If services don't have unit tests, the compile-only smoke style is fine — the integration tests are what really cover behavior.

### 9. Verify

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass.

### 10. Commit slices

Commit each verified slice without pausing — feature branch in a worktree:

1. `feat(types): add SongSource enum for library→queue dispatch` — `data/src/types/song_source.rs` + `mod.rs` wiring. Tests if any inline.
2. `feat(backend): add LibraryOrchestrator borrow-handle for entity resolution` — orchestrator type, accessor on AppService, mod.rs wiring. Doc comments quote the audit's recommendation.
3. `test(library_orchestrator): cover resolve dispatch + on-demand API construction` — the unit tests. (Combine with #2 if test count is small.)

Each slice runs the four-step verify. Skip the `Co-Authored-By` trailer per global instructions.

### 11. Report

End with: commits (refs + subjects), `wc -l data/src/backend/library_orchestrator.rs` final value, `wc -l data/src/types/song_source.rs` final value, the `AuthGateway` accessor shape your sub-agent confirmed (one line), and a one-line note on whether you added a new `client_arc()`-style helper or used an existing one.

## What NOT to touch

- Any `play_*` / `add_*` / `insert_*` / `play_next_*` / `*_batch` / `*_song_by_id` method body in `app_service.rs`. Those are Lanes C and D.
- The private `AppService::load_genre_songs` / `load_playlist_songs` / `play_next_songs` helpers (lines 724-777) — they stay until Lane D deletes them.
- `data/src/backend/queue_orchestrator.rs` — Lane B's territory.
- `src/update/*` — UI is unchanged.
- `.agent/rules/` — out of scope.

## If blocked

- If `AuthGateway` has no client accessor and adding one feels invasive: stop, report the surrounding shape, propose option (b) from step 2 (parameter-passing), wait for confirmation.
- If `cargo test` fails on baseline before you add anything: stop, do not proceed; this means the worktree is on a broken commit.
- If clippy flags `unused_async` on a resolver method (because the body is `Ok(songs)` for `Preloaded` after match-flattening): apply the minimal fix, do not paper over with `#[allow]`.
````

### lane-b-queue

worktree: ~/nokkvi-appservice-b
branch: refactor/appservice-queue-orchestrator
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane B of the AppService entity×verb split — add `QueueOrchestrator` borrow-handle. Pure additive — does NOT migrate any existing AppService method bodies.

Plan doc: /home/foogs/nokkvi/.agent/plans/appservice-orchestrator-split.md (sections 2.3, 4 "Lane B").

Working directory: ~/nokkvi-appservice-b (this worktree). Branch: refactor/appservice-queue-orchestrator. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `c45258b` or a descendant on `main`.
- `ls data/src/backend/queue_orchestrator.rs 2>&1` reports "No such file".
- `rg -n 'pub mod queue_orchestrator' data/src/backend/mod.rs` returns nothing.

### 2. Sub-agent dispatch — confirm queue + playback primitives

Before writing the orchestrator, dispatch ONE Explore sub-agent:

```
Agent({
  description: "Confirm QueueService + PlaybackController primitives",
  subagent_type: "Explore",
  prompt: "I need exact signatures (and a 3-5 line summary of what each does) for these methods in /home/foogs/nokkvi/data/src/backend/. Return them in a clean table.

  PlaybackController (data/src/backend/playback_controller.rs):
  - play_songs_from_index — likely around lines 596-640
  - play_song_from_queue — likely around lines 648-697
  
  QueueService (data/src/backend/queue.rs):
  - add_songs — likely around lines 147-160
  - insert_songs_at — likely around lines 284-299
  - get_songs — somewhere in the file
  - get_current_index — somewhere in the file (returns Option<usize> or usize?)
  - refresh_from_queue — somewhere in the file (does this exist as public? if not, what's the equivalent?)
  
  AppService::play_next_songs (data/src/backend/app_service.rs:759-777) — read the full body and report what sequence of QueueService/PlaybackController calls it makes. The QueueOrchestrator::play_next will replicate this sequence.

  Report each method's full signature, async/sync, return type, and a one-sentence summary. Under 600 words."
})
```

Wait for the sub-agent to return before proceeding. The exact signatures inform the QueueOrchestrator method bodies.

### 3. Write `data/src/backend/queue_orchestrator.rs`

Per plan §2.3. Structure:
- `pub struct QueueOrchestrator<'a> { queue: &'a QueueService, playback: &'a PlaybackController }`.
- `pub(crate) fn new(queue, playback)` constructor.
- Five verb methods: `play(songs, start_index)`, `enqueue(songs)`, `enqueue_and_play(songs)`, `insert_at(songs, position)`, `play_next(songs)`. All `pub async fn` returning `Result<()>`.

`play_next` body must match what your sub-agent found in today's `AppService::play_next_songs` (lines 759-777) exactly — same sequence, same arithmetic, same refresh call (or whatever the sub-agent confirmed). If the existing helper does something the plan doesn't anticipate (e.g. extra logging, atomic flag flip, side effect), preserve it.

`enqueue_and_play` snapshots `self.queue.get_songs().len()` BEFORE `add_songs`, then plays the snapshotted index — matches the existing `add_album_and_play` pattern (app_service.rs:453-466).

Use `anyhow::Result`. Match logging verbosity to existing peer methods.

### 4. Wire QueueOrchestrator into backend/mod.rs

Add at the appropriate position (alphabetical, near `library_orchestrator` if Lane A's already merged — otherwise at the natural alphabetical slot):
```rust
pub mod queue_orchestrator;
pub use queue_orchestrator::QueueOrchestrator;
```

If Lane A has not yet merged, `mod.rs` will get a clean addition; if it has merged, this is one extra line, mechanical rebase.

### 5. Add accessor to AppService

In `data/src/backend/app_service.rs`, add a new `impl AppService` block under a fresh banner:

```rust
// === Queue orchestrator accessor ===
impl AppService {
    /// Borrow-handle for queue-mutation verbs. Holds no state.
    pub(crate) fn queue_orchestrator(&self) -> QueueOrchestrator<'_> {
        QueueOrchestrator::new(&self.queue_service, &self.playback)
    }
}
```

**Place this `impl` block in its own location** (not the main `impl AppService`, not next to Lane A's `library_orchestrator()` block) so the two parallel-lane diffs land at different line ranges and rebase mechanically. A safe spot: just before the existing `genres_api()` / `playlists_api()` accessors, OR right after them — Lane A picks one, you pick the other.

### 6. Tests

Add `#[cfg(test)] mod tests` inside `queue_orchestrator.rs`. Cover:
- `play_replaces_queue_and_starts_at_index`
- `enqueue_appends_without_changing_current`
- `enqueue_and_play_jumps_to_first_appended`
- `enqueue_and_play_noop_on_empty_input`
- `insert_at_passes_position_through`
- `play_next_inserts_at_current_plus_one`
- `play_next_when_no_current_inserts_at_zero`

Match the test pattern in `data/src/services/queue/*.rs` for setup. If those tests use a fixture `QueueManager::new_for_test()` style helper, reuse it. If existing tests of `QueueService` are sparse, compile-only smoke is acceptable — match what's there.

### 7. Verify

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass.

### 8. Commit slices

1. `feat(backend): add QueueOrchestrator with five queue-mutation verbs` — orchestrator type, mod.rs wiring, AppService accessor.
2. `test(queue_orchestrator): cover play / enqueue / enqueue_and_play / insert_at / play_next` — unit tests.

(Combine into one commit if test count is small.) Each slice runs the four-step verify. Skip the `Co-Authored-By` trailer.

### 9. Report

End with: commits (refs + subjects), `wc -l data/src/backend/queue_orchestrator.rs` final value, the exact signatures your sub-agent confirmed for `QueueService::get_current_index` and `refresh_from_queue` (one line each), and one sentence on whether `play_next`'s body matches the existing `AppService::play_next_songs` (lines 759-777) verbatim.

## What NOT to touch

- Any existing AppService method body — those are Lanes C and D.
- The private `AppService::play_next_songs` helper (line 759) — it stays until Lane D deletes it.
- `data/src/types/song_source.rs` / `library_orchestrator.rs` — Lane A's territory. Your orchestrator does NOT use `SongSource` directly; it accepts `Vec<Song>`.
- `src/update/*` — UI is unchanged.
- `.agent/rules/` — out of scope.

## If blocked

- If `QueueService::get_current_index` returns something other than `Option<usize>` (e.g. `usize` with a sentinel, or a `QueueState` struct): adjust `play_next` to match. Report the actual shape in your final report.
- If `refresh_from_queue` doesn't exist publicly: check whether `insert_songs_at` already does the refresh internally. If yes, drop the explicit refresh from `play_next`. If no, escalate — the existing `AppService::play_next_songs` body is the source of truth.
- If clippy flags `unused_async` on `enqueue_and_play` (because `add_songs` early-returns and isn't `await`ed in the empty-input branch): use the early-return-Ok shape shown in plan §2.3.
````

### lane-c-play

worktree: ~/nokkvi-appservice-c
branch: refactor/appservice-play-family
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane C of the AppService entity×verb split — migrate the 9 `play_*` methods on `AppService` to delegate through `LibraryOrchestrator` + `QueueOrchestrator`. Preserves all public method names + signatures.

Plan doc: /home/foogs/nokkvi/.agent/plans/appservice-orchestrator-split.md (sections 2.4, 4 "Lane C").

Working directory: ~/nokkvi-appservice-c (this worktree). Branch: refactor/appservice-play-family. The worktree is already created — do NOT run `git worktree add`.

## Dependency

This lane depends on **Lanes A and B**: `LibraryOrchestrator`, `QueueOrchestrator`, `library_orchestrator()` accessor, and `queue_orchestrator()` accessor must exist on `AppService` before this lane's work compiles.

**Before starting**: check whether Lanes A and B have merged to `main`:
```bash
git fetch origin main
git log origin/main --oneline | head -10
```

If both lanes are on `main`: rebase your branch onto `main` (`git rebase origin/main`) and proceed.

If only one or neither has merged: pull their feature branches into a temporary integration commit on top of your branch (`git fetch origin refactor/appservice-library-orchestrator refactor/appservice-queue-orchestrator && git merge --no-edit origin/refactor/appservice-library-orchestrator origin/refactor/appservice-queue-orchestrator`). Document this in your final report. Do NOT push the integration merge — at final-merge-to-main time, you'll rebase cleanly once A and B are on `main`.

If neither lane has produced a branch yet: STOP. Wait for them. This lane cannot start work without Lanes A and B's types in scope.

## What to do

### 1. Verify baseline + dependency

- `rg -n 'pub fn library_orchestrator|pub\(crate\) fn library_orchestrator' data/src/backend/app_service.rs` returns 1 match (Lane A landed).
- `rg -n 'pub fn queue_orchestrator|pub\(crate\) fn queue_orchestrator' data/src/backend/app_service.rs` returns 1 match (Lane B landed).
- `rg -n 'pub use song_source::SongSource' data/src/types/mod.rs` returns 1 match.
- `cargo build` succeeds before any modification.

### 2. Sub-agent dispatch — produce the 9 migration patches

Dispatch ONE general-purpose sub-agent to draft the new bodies. This parallelizes the mechanical translation:

```
Agent({
  description: "Draft 9 play_* migration patches",
  subagent_type: "general-purpose",
  prompt: "Read /home/foogs/nokkvi/data/src/backend/app_service.rs lines 258-358. For EACH of these 9 methods, produce the EXACT new body using the orchestrator pattern. Output should be a numbered list, one method per entry, with the method name + full new body wrapped in a Rust code fence.

  Methods (line numbers are at the `pub async fn` line):
  1. play_album (258) — `let songs = self.library_orchestrator().resolve_album(album_id).await?; self.queue_orchestrator().play(songs, 0).await`
  2. play_album_from_track (266) — resolve_album + clamp track_idx + play(songs, start). Read the existing body to confirm whether clamping happens caller-side or inside play_songs_from_index; preserve the visible-clamp pattern.
  3. play_artist (274) — resolve_artist + play(songs, 0).
  4. play_genre (282) — resolve_genre + play(songs, 0).
  5. play_genre_random (290) — resolve_genre + early-return-empty + rand::random_range(0..songs.len()) + play(songs, idx). PRESERVE the existing empty-check, log lines, and rand call shape — read lines 290-297 carefully and replicate the exact randomization step.
  6. play_artist_random (301) — same shape as #5 but resolve_artist. Read lines 301-308.
  7. play_playlist (313) — resolve_playlist + play(songs, 0).
  8. play_playlist_from_track (321) — resolve_playlist + clamp + play. Read lines 321-328.
  9. play_songs (350) — direct: `self.queue_orchestrator().play(songs, start_index).await`. Read lines 350-358 to confirm no extra step is being lost.

  For each new body, also list any imports that need to be added (e.g. SongSource is not used directly here — only the typed resolve_* methods on the orchestrator, so the SongSource import probably doesn't change). Confirm whether `rand::random_range` is already imported in app_service.rs (grep for it).

  Don't write the file. Just produce the patches as a structured report under 1200 words."
})
```

Wait for the sub-agent to return. Review each proposed body against the actual current body in the file — if any pre-existing logic (a tracing call, a metric increment, a side-effect) is missing from the proposed body, flag it before applying.

### 3. Apply the migrations

Use the Edit tool, one method at a time. Each edit replaces the OLD body (between `pub async fn name(...) -> Result<()> {` and the matching `}`) with the NEW body produced by the sub-agent. **Keep the function signature and doc comment line-for-line identical** — only the body changes.

After each method's migration:
```bash
cargo build
```
to catch type errors early. If a build fails, undo that one method's edit, investigate, fix, re-apply.

### 4. Imports

Add `use crate::types::song_source::SongSource;` to `data/src/backend/app_service.rs` if **any** of the new bodies actually constructs a `SongSource` enum value. Per the sub-agent's report, the typed `resolve_album` / `resolve_artist` / `resolve_genre` / `resolve_playlist` methods are preferred over `resolve(SongSource::X(id))` for the per-entity calls, so the import may not be needed.

### 5. Verify

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass. Specifically check:

- `cargo test --bin nokkvi -- play_album` (and `play_artist`, `play_genre`, `play_playlist`) — any handler tests touching these methods must pass without modification.
- `cargo test -p nokkvi-data` — the orchestrator unit tests Lanes A and B added must still pass.
- The 8 UI call sites listed in plan §4 Lane C must compile (this is implicit in `cargo build` succeeding; no separate check needed).

### 6. Commit slices

Commit each verified slice. Suggested cadence:

1. `refactor(app_service): migrate play_album + play_album_from_track to LibraryOrchestrator` — 2 method bodies.
2. `refactor(app_service): migrate play_artist + play_artist_random to LibraryOrchestrator` — 2 method bodies.
3. `refactor(app_service): migrate play_genre + play_genre_random to LibraryOrchestrator` — 2 method bodies.
4. `refactor(app_service): migrate play_playlist + play_playlist_from_track to LibraryOrchestrator` — 2 method bodies.
5. `refactor(app_service): trivialize play_songs to QueueOrchestrator delegation` — 1 method body.

Each slice: full four-step verify. Skip the `Co-Authored-By` trailer.

### 7. Report

End with: commits (refs + subjects), `wc -l data/src/backend/app_service.rs` delta (expected ~50-80 LOC reduction in the play family alone), `grep -c 'fn play_album\|fn play_album_from_track\|fn play_artist\|fn play_artist_random\|fn play_genre\|fn play_genre_random\|fn play_playlist\|fn play_playlist_from_track\|fn play_songs' data/src/backend/app_service.rs` final value (should equal `9` — all 9 methods preserved), and a list of any tests that needed adjustment (should be empty list; if not, document why).

## What NOT to touch

- The `add_*_to_queue` / `add_*_and_play` / `insert_*_at_position` / `play_next_*` families (lines 368-820) — Lane D's territory.
- The private `AppService::load_genre_songs` / `load_playlist_songs` / `play_next_songs` helpers — they're still callers' targets in lines you DON'T touch (used by Lane D's region). They stay until Lane D deletes them.
- The `*_batch` / `*_song_by_id` / `play_next_preloaded` methods — out of scope (see plan §2.4).
- `LibraryOrchestrator` / `QueueOrchestrator` definitions — Lanes A and B's territory.
- `src/update/*` — UI doesn't change.

## If blocked

- If `cargo build` fails because `LibraryOrchestrator::resolve_genre` has a different signature than expected (e.g. takes `&str` vs `String`): adjust the call sites to match Lane A's actual implementation; do NOT modify Lane A's orchestrator from this branch.
- If a `play_*_random` body has additional logic the sub-agent missed (e.g. a metric event, a `tracing::info!` with track count): preserve it verbatim — log content is part of observable behavior.
- If clippy flags `useless_conversion` or similar on the new bodies: minimal fix, no broad `#[allow]`.
- If a test fails after migrating `play_album_from_track` due to clamp behavior: re-read the existing body's clamp arithmetic carefully — `track_idx.min(songs.len() - 1)` underflows if `songs.is_empty()`. The plan suggests `track_idx.min(songs.len().saturating_sub(1))`; if today's body uses different arithmetic, match it exactly.
````

### lane-d-mutate

worktree: ~/nokkvi-appservice-d
branch: refactor/appservice-mutation-families
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane D of the AppService entity×verb split — migrate the 16 entity×verb mutation methods + 4 song-keyed delegations on `AppService`, and DELETE the 3 now-obsolete private helpers (`load_genre_songs`, `load_playlist_songs`, `play_next_songs`). Preserves all public method names + signatures.

Plan doc: /home/foogs/nokkvi/.agent/plans/appservice-orchestrator-split.md (sections 2.4, 4 "Lane D").

Working directory: ~/nokkvi-appservice-d (this worktree). Branch: refactor/appservice-mutation-families. The worktree is already created — do NOT run `git worktree add`.

## Dependency

Same as Lane C — depends on Lanes A and B. **Run the same dependency check** (see Lane C step "Dependency" + step 1). If Lane C is also in flight, that's fine — Lane D edits a disjoint line range.

**Before starting**: confirm Lanes A and B's branches are reachable. If they've merged, rebase. If they're on feature branches, integration-merge them locally. If they don't exist yet, STOP.

## What to do

### 1. Verify baseline + dependency

- Same as Lane C step 1.
- Additionally: `wc -l data/src/backend/app_service.rs` should report ~970 (or slightly more if Lanes A and B's accessors landed — adds ~10-15 LOC).
- `grep -n 'fn load_genre_songs\|fn load_playlist_songs\|fn play_next_songs' data/src/backend/app_service.rs` should report all 3 helpers present.

### 2. Sub-agent dispatch — produce migration patches in two parallel batches

Two sub-agents in parallel — one for the add+insert families, one for the play_next family + helper deletions. Issue both in a single tool-call message:

```
Agent({
  description: "Draft add/insert migration patches",
  subagent_type: "general-purpose",
  prompt: "Read /home/foogs/nokkvi/data/src/backend/app_service.rs lines 336-650. For EACH of these methods, produce the EXACT new body using the orchestrator pattern. Output as a numbered list, method name + new body in a Rust code fence.

  Methods:
  1. load_playlist_into_queue (336) — resolve_playlist + the existing post-load step (read the current body to identify what's after the load — preserve that step).
  2. add_album_to_queue (368) — resolve_album + enqueue.
  3. add_artist_to_queue (379) — resolve_artist + enqueue.
  4. add_song_to_queue (390) — direct: enqueue(vec![song]).
  5. add_song_and_play (399) — direct: enqueue_and_play(vec![song]).
  6. add_song_to_queue_by_id (411) — KEEP CURRENT BODY (find-by-id is bespoke). Read it; if any internal call is to load_genre_songs / load_playlist_songs / play_next_songs (the helpers Lane D deletes), flag it.
  7. add_genre_to_queue (425) — resolve_genre + enqueue.
  8. add_playlist_to_queue (436) — resolve_playlist + enqueue.
  9. add_album_and_play (453) — resolve_album + enqueue_and_play.
  10. add_artist_and_play (469) — resolve_artist + enqueue_and_play.
  11. add_genre_and_play (488) — resolve_genre + enqueue_and_play.
  12. add_playlist_and_play (507) — resolve_playlist + enqueue_and_play.
  13. insert_album_at_position (533) — resolve_album + insert_at(songs, position).
  14. insert_artist_at_position (547) — resolve_artist + insert_at(songs, position).
  15. insert_song_at_position (561) — direct: insert_at(vec![song], position).
  16. insert_song_by_id_at_position (574) — KEEP CURRENT BODY.
  17. insert_genre_at_position (598) — resolve_genre + insert_at(songs, position).

  For each, confirm: (a) exact new body, (b) whether the existing body has any pre-existing tracing/log/side-effect that the new body must preserve, (c) whether the existing body uses the about-to-be-deleted private helpers — if yes, the new body's resolve_* call replaces that path. Under 1500 words."
})

Agent({
  description: "Draft play_next + helper-deletion patches",
  subagent_type: "general-purpose",
  prompt: "Read /home/foogs/nokkvi/data/src/backend/app_service.rs lines 720-825. Produce migration patches and identify deletions.

  Migration patches (5 methods):
  1. play_next_album (780) — resolve_album + queue_orchestrator().play_next(songs).
  2. play_next_song_by_id (786) — KEEP CURRENT BODY (find-by-id is bespoke). Read it.
  3. play_next_artist (798) — resolve_artist + play_next.
  4. play_next_genre (804) — resolve_genre + play_next.
  5. play_next_playlist (810) — resolve_playlist + play_next.
  6. play_next_preloaded (816) — direct: queue_orchestrator().play_next(songs).

  Deletions (3 private helpers):
  1. load_genre_songs (724-728) — DELETE. Confirm by grep that no remaining caller exists in app_service.rs after the migration patches above are applied (the only callers should be the methods being migrated; once they're delegating through library_orchestrator().resolve_genre, this helper has zero callers).
  2. load_playlist_songs (739-745) — DELETE. Same caller analysis.
  3. play_next_songs (759-777) — DELETE. Same caller analysis. Body of this helper is what QueueOrchestrator::play_next replicates.

  Report: (a) the 6 migration patches with full new bodies in code fences, (b) confirm zero remaining callers for each of the 3 helpers post-migration, (c) flag any caller you didn't expect. Under 800 words."
})
```

Wait for both sub-agents to return. Cross-check their findings against the actual file before applying.

### 3. Apply migrations in waves

The lane has 22 patches + 3 deletions. To keep `cargo build` green throughout:

**Wave 1 — `add_*_to_queue` family** (5 methods):
Apply patches for `add_album_to_queue`, `add_artist_to_queue`, `add_song_to_queue`, `add_genre_to_queue`, `add_playlist_to_queue`. Build + test + clippy + fmt-check. Commit.

**Wave 2 — `add_*_and_play` family** (5 methods):
Apply patches for `add_song_and_play`, `add_album_and_play`, `add_artist_and_play`, `add_genre_and_play`, `add_playlist_and_play`. (Skip `add_song_to_queue_by_id` — keep current body.) Verify. Commit.

**Wave 3 — `insert_*_at_position` family** (4 methods):
Apply patches for `insert_album_at_position`, `insert_artist_at_position`, `insert_song_at_position`, `insert_genre_at_position`. (Skip `insert_song_by_id_at_position` — keep current body.) Verify. Commit.

**Wave 4 — `play_next_*` family** (5 methods):
Apply patches for `play_next_album`, `play_next_artist`, `play_next_genre`, `play_next_playlist`, `play_next_preloaded`. (Skip `play_next_song_by_id` — keep current body.) Verify. Commit.

**Wave 5 — `load_playlist_into_queue` migration** (1 method):
Apply patch. Verify. Commit.

**Wave 6 — Delete the 3 private helpers**:
After all the entity×verb migrations are in, the 3 helpers should have zero callers in `app_service.rs`. Verify with grep:
```bash
rg -n 'load_genre_songs|load_playlist_songs|play_next_songs' data/src/backend/app_service.rs
```
Should report only the function definitions themselves (the `async fn ...` lines). Delete those 3 functions. Verify. Commit.

### 4. Verify (after every wave)

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass at every wave boundary. If a test fails, undo the wave's edits, investigate, re-apply.

### 5. Commit slices (per wave above)

1. `refactor(app_service): migrate add_*_to_queue family to QueueOrchestrator::enqueue` — wave 1.
2. `refactor(app_service): migrate add_*_and_play family to QueueOrchestrator::enqueue_and_play` — wave 2.
3. `refactor(app_service): migrate insert_*_at_position family to QueueOrchestrator::insert_at` — wave 3.
4. `refactor(app_service): migrate play_next_* family to QueueOrchestrator::play_next` — wave 4.
5. `refactor(app_service): delegate load_playlist_into_queue resolve to LibraryOrchestrator` — wave 5.
6. `refactor(app_service): delete obsolete private load_genre_songs / load_playlist_songs / play_next_songs helpers` — wave 6.

Each slice: full four-step verify. Skip the `Co-Authored-By` trailer.

### 6. Report

End with: commits (refs + subjects), `wc -l data/src/backend/app_service.rs` delta (expected ~150-200 LOC reduction), `grep -c 'fn load_genre_songs\|fn load_playlist_songs\|fn play_next_songs' data/src/backend/app_service.rs` final value (should equal `0`), `grep -c 'fn add_album_to_queue\|fn add_artist_to_queue\|fn add_genre_to_queue\|fn add_playlist_to_queue\|fn add_album_and_play\|fn add_artist_and_play\|fn add_genre_and_play\|fn add_playlist_and_play\|fn insert_album_at_position\|fn insert_artist_at_position\|fn insert_genre_at_position\|fn play_next_album\|fn play_next_artist\|fn play_next_genre\|fn play_next_playlist' data/src/backend/app_service.rs` final value (should equal `15`).

## What NOT to touch

- The `play_*` family (lines 258-358) — Lane C's territory.
- The `*_batch` methods (lines 828-946) — out of scope.
- The `*_song_by_id` methods (lines 411, 574, 786) — KEEP CURRENT BODY per plan §2.4.
- `remove_queue_songs` (line 948) — out of scope.
- `LibraryOrchestrator` / `QueueOrchestrator` definitions — Lanes A and B's territory.
- `src/update/*` — UI doesn't change.

## If blocked

- If a `*_song_by_id` method's body uses one of the 3 helpers to be deleted: the body is bespoke and stays; replace just the helper-call line with a direct `library_orchestrator().resolve_*` call (so the deletion can proceed). Document this in the wave-6 commit body.
- If a wave's verify fails because a UI call site's `?` propagation behaves differently (the orchestrator returns `anyhow::Error` shape vs the old method body): inspect the returned error type. The orchestrator should return the same `anyhow::Result<()>` shape — if not, fix the orchestrator (escalate to Lanes A/B) rather than masking at the AppService boundary.
- If the helper-deletion grep finds an unexpected caller (anywhere outside the migrated methods): STOP, report. Do not delete a helper that still has external callers.
- If clippy flags `unused_self` on `add_song_to_queue` after it becomes a 1-line delegation: that's fine — the method preserves the public API; `#[allow(clippy::unused_self)]` is acceptable in this specific case ONLY IF clippy actually fires (unlikely, since `enqueue` borrows `self` transitively).
````

### lane-e-followup

worktree: ~/nokkvi-appservice-e
branch: refactor/appservice-followup
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane E of the AppService entity×verb split — UI smoke verification, test-mirror search via sub-agent, audit-tracker flip. This lane runs concurrent with A-D and finalizes once they've landed.

Plan doc: /home/foogs/nokkvi/.agent/plans/appservice-orchestrator-split.md (sections 4 "Lane E", 5).

Working directory: ~/nokkvi-appservice-e (this worktree). Branch: refactor/appservice-followup. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` shows `c45258b` or descendant.
- `cat .agent/audit-progress.md | grep -A1 '§7 #7\|§7 row 7\|`§7 #7`\|orchestrator'` should show §7 row 7 status as `❌ open`.

### 2. Sub-agent dispatch — three parallel research tasks

Issue all three in a single tool-call message:

```
Agent({
  description: "Search for entity×verb test mirrors",
  subagent_type: "Explore",
  prompt: "Search /home/foogs/nokkvi/src/update/tests/ for any test set that mirrors the play_album / play_artist / play_genre / play_playlist axis (or the add_*_to_queue, insert_*_at_position, play_next_* siblings) as parallel test bodies — i.e. tests that call shell.play_album(...) and shell.play_artist(...) etc. with near-identical setup.

  Exclude /home/foogs/nokkvi/src/update/tests/navigation.rs — it's covered by a separate refactor (.agent/plans/pending-expand-dedup.md).

  For each mirror found, report: file:line of each parallel test, test names, the variation axis (entity? verb? both?), and a one-sentence judgment on whether a for_each_library_entity!-style macro would be a clean fold. If no mirror is found across update/tests/, say so explicitly. Under 600 words."
})

Agent({
  description: "Verify 22 UI call sites still pattern-match",
  subagent_type: "Explore",
  prompt: "Read these 22 UI call sites in /home/foogs/nokkvi/ and confirm each is a thin shell.PLAY_METHOD(args).await call, exactly as today. Report any site where the call shape is non-trivial (e.g. wrapped in a closure that does extra work, error handling that depends on a specific Result type, etc.):

  src/update/albums.rs:591 — shell.insert_album_at_position(&id, position).await
  src/update/albums.rs:601 — shell.add_album_to_queue(&id).await
  src/update/albums.rs:614 — shell.add_album_and_play(&id).await
  src/update/albums.rs:625 — shell.play_album(&id).await
  src/update/albums.rs:782 — shell.play_album_from_track(&album_id, track_idx).await
  src/update/artists.rs:433 — shell.insert_artist_at_position(&id, position).await
  src/update/artists.rs:443 — shell.add_artist_to_queue(&id).await
  src/update/artists.rs:456 — shell.add_artist_and_play(&id).await
  src/update/artists.rs:467 — shell.play_artist(&id).await
  src/update/artists.rs:502 — shell.add_album_to_queue(&album_id).await
  src/update/artists.rs:508 — shell.play_album(&album_id).await
  src/update/genres.rs:268 — shell.insert_genre_at_position(&name, pos).await
  src/update/genres.rs:276 — shell.add_genre_to_queue(&genre_name).await
  src/update/genres.rs:287 — shell.add_genre_and_play(&name).await
  src/update/genres.rs:294 — shell.play_genre(&genre_name).await
  src/update/genres.rs:331 — shell.add_album_to_queue(&album_id).await
  src/update/genres.rs:337 — shell.play_album(&album_id).await
  src/update/playlists.rs:173 — shell.add_playlist_to_queue(&playlist_id).await
  src/update/playlists.rs:189 — shell.add_playlist_and_play(&playlist_id).await
  src/update/playlists.rs:208 — shell.play_playlist(&playlist_id).await
  src/update/roulette.rs:356 — shell.play_genre_random(&name).await
  src/update/roulette.rs:368 — shell.play_artist_random(&id).await

  Confirm each is a thin call. Flag any that would behave subtly differently if the underlying method's error type changed (it shouldn't; orchestrator returns the same anyhow::Result<()>). Under 800 words."
})

Agent({
  description: "Draft audit-tracker §7 #7 flip",
  subagent_type: "general-purpose",
  prompt: "Read /home/foogs/nokkvi/.agent/audit-progress.md in full. Locate §7 row 7 (the AppService LibraryOrchestrator + QueueOrchestrator split row) and §3 row 4 (which references §7 row 7 as 'AppService entity × verb matrix').

  Produce the EXACT post-refactor diff to:
  1. Flip §7 row 7 to '✅ done' with the format used by other ✅ rows (look at row 1, row 2, row 10, row 12 for shape — they typically include: lane breakdown sentence, commit refs in monospace inline, evidence-line listing the deleted helpers + the new orchestrator types).
  2. Update §3 row 4 status to ✅ done with note 'Same as §7 #7'.
  3. In the 'Quick-pick: highest-leverage open items' section at the bottom, update items #1 and #2 to mention that §7 #5 (ItemKind) and §7 #8 (LoaderTarget) are now more accessible because SongSource exists.

  Use placeholder commit refs like {LANE_A_REFS}, {LANE_B_REFS}, {LANE_C_REFS}, {LANE_D_REFS} that I'll fill in. Output as a unified diff or as before/after markdown blocks. Under 700 words."
})
```

### 3. Wait for Lanes A-D to land

Lane E's audit-tracker flip needs the actual commit refs from Lanes A-D. Until they merge:
- Use the test-mirror-search agent's findings to start drafting any test-fold work (separate small commit slices).
- Use the UI-verification agent's findings to flag any call sites that need attention post-merge (none expected, but document any flags).

### 4. (Conditional) Fold a test mirror if one was found

If the test-mirror-search agent reports a parallel-tests mirror outside `tests/navigation.rs` (e.g., a `tests/play_actions.rs` with mirrored album/artist/genre/playlist setups):

a. Read those tests in full.
b. Decide whether a `for_each_library_entity!` macro fold is clean. Consult `.agent/plans/pending-expand-dedup.md` §2.3 for the macro pattern (similar structure: entity binding macro + scenario kernel). If the test count is < 10, leave them prose — the fold isn't worth it.
c. If folding: write the macro + kernel, migrate the tests, verify with `cargo test`. Keep the old test names as `mod <entity>::<scenario>` paths.
d. If not folding: skip step 4 entirely. Document the decision in the final report.

### 5. Apply the audit-tracker flip

Once Lanes A-D have landed and you have their commit refs:

a. Replace `{LANE_A_REFS}` / `{LANE_B_REFS}` / `{LANE_C_REFS}` / `{LANE_D_REFS}` in the agent's draft with the actual short SHAs (e.g., `7f3a012..a1b2c3d`).
b. Apply the edit to `.agent/audit-progress.md` using the Edit tool.
c. Verify the markdown rendering by re-reading the file — make sure the row format matches sibling ✅ rows (commit refs in backticks, evidence sentences are complete).

### 6. Verify

```bash
cargo build  # sanity
cargo test   # sanity, unchanged tests pass
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

Lane E shouldn't change Rust code unless step 4 applied. If it did, all four checks must pass.

### 7. Commit slices

Variable depending on what step 4 did:

If step 4 applied (test fold):
1. `test(<area>): collapse <entity>×<verb> mirror via for_each_library_entity! macro` — the fold.

Always:
- `docs(audit): flip §7 #7 to ✅ done after AppService orchestrator split lands` — the audit-tracker update with all four lane refs.

Each slice: four-step verify. Skip the `Co-Authored-By` trailer.

### 8. Report

End with: commits (refs + subjects), test-mirror finding (one sentence — found / not found, where), UI-verification finding (one sentence — all 22 sites trivial / N flagged), and the audit-tracker line for §7 #7 verbatim post-flip.

## What NOT to touch

- `data/src/backend/app_service.rs` — Lanes C and D's territory.
- `data/src/backend/library_orchestrator.rs` / `queue_orchestrator.rs` — Lanes A and B's territory.
- `data/src/types/song_source.rs` — Lane A's territory.
- `src/update/tests/navigation.rs` — covered by `pending-expand-dedup.md`, not this plan.
- Any UI call site (only verify their shape; do not modify them).
- `.agent/rules/` files — out of scope; rules-doc syncing is `/sync-rules`'s job.

## If blocked

- If the test-mirror-search agent finds a mirror that's larger than expected (>20 tests): consider extracting it into a separate plan rather than folding inline. Document in the final report.
- If a UI call site is flagged as non-trivial (e.g., wraps the call in a closure that does post-processing): re-read the surrounding handler to confirm the orchestrator-routed body still produces the same observable behavior. If genuinely different, escalate before merging Lanes C/D.
- If Lanes A-D don't all merge cleanly (e.g., Lane B has a conflict with Lane A's mod.rs addition that the merger didn't catch): coordinate with the lane owner; do not paper over with `git merge -X theirs`. The conflict resolution is mechanical — one extra `pub mod` line, one extra accessor — but it should be done explicitly.
- If `.agent/audit-progress.md`'s sibling ✅ rows have evolved a different format than the ones the agent referenced: match the most recent format.
````
