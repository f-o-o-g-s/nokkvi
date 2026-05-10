# `enum ItemKind` migration — fanout plan (§7 #5 / Drift #2 / DRY #6)

Closes `.agent/audit-progress.md` §7 #5 / §4 Drift #2. Today the entity kind that drives star/rating revert dispatch flows through the codebase as `item_type: &str` (or `&'static str`) carrying one of `"album"`, `"artist"`, `"song"`. The two match arms in `src/update/components.rs:786-791` and `:796-802` use `_ => …Song` fallbacks that silently route any unknown string (typo, future variant) to the `Song` arm. This plan replaces the string with a typed `enum ItemKind`, converts the wildcards to exhaustive matches, and threads the type through every action variant + helper + hotkey site without changing UI behavior.

Last verified baseline: **2026-05-09, `main @ HEAD = 4edc0ec`** (`refactor/appservice-*` worktrees merged + cleaned; §7 #7 + batch tidy fully landed; `data/src/types/song_source.rs` and `data/src/types/batch.rs` present; no `enum ItemKind` defined anywhere; no `item_kind` identifier in either crate).

Source reports: `~/nokkvi-audit-results/{_SYNTHESIS,drift-magic,drift-triangle,dry-handlers}.md`.

---

## 1. Goal & rubric

The audit's framing (`drift-magic.md` §3 lines 129-162):

> "Appears in **10 files**; type is `&str` (or `&'static str`), so a typo compiles silently. The match arms in `src/update/components.rs:783-789` and `:793-799` use a `_ =>` fallback that **routes any unknown string to the `Song` variant** — meaning a future `"playlist"` rating call (or just a typo'd `"alubm"`) would silently rate something as a song."

And the recommendation (`_SYNTHESIS.md` §7 row 5):

> "`enum ItemKind { Album, Artist, Song, Playlist }` with `api_str(self) -> &'static str`. Replace every `&str` parameter and the catch-all `_ =>` with exhaustive matches."

Today the bug class is real and unguarded. Two of the four enum variants the audit names (`Album`, `Artist`, `Song`) reach the dispatch helpers; `Playlist` is reserved for the future (the audit's "future `"playlist"` rating call" hypothetical). Today's hotkey rating handler typo'ing `"alubm"` would silently dispatch a `Song` rating update — the user would star/rate the song that happens to be selected, and the album would stay unchanged. The starred state across the local entity-snapshot lists would diverge from the server.

Rubric (in order):

1. **Bug-class prevention.** Adding a new `ItemKind` variant is a compiler error at every match site; typing `"alubm"` no longer compiles; the two `_ => …Song` wildcards in `components.rs` become exhaustive `match self { Album|Artist|Song|Playlist => … }` arms. This is the headline.
2. **Public API stable at the data crate boundary.** `api::star::star_item / unstar_item / toggle_star / api::rating::set_rating` keep their signatures — the Subsonic wire format is unchanged. The `item_type: &str` parameter on the API functions is purely an error-message label (not a wire param: see §3); we widen it to `impl Into<&'static str>` via `ItemKind::api_str()` at the call site rather than reshape the API.
3. **Test signal preservation.** Every existing test passes unchanged. No tests today exercise the wildcard fallback (it's silent); we add a `kind_revert_message` test that pins `Album → AlbumStarredStatusUpdated`, `Artist → ArtistStarredStatusUpdated`, `Song → SongStarredStatusUpdated`, `Playlist → SongStarredStatusUpdated` (or the chosen forward-compat behavior) so the table is exhaustive.
4. **Agent-friendliness over LOC.** Per the user's standing preference, refactors are weighted by how well they prevent bug classes AI agents are prone to (silent defaults, copy-paste drift, magic indices). LOC delta is incidental. The drift-anchor commit pattern from §7 #2 (`61593bc` — `View::ALL` + paired `const _:` length asserts) is the precedent.
5. **Genre asymmetry stays visible.** `ItemKind` does NOT have a `Genre` variant. `Genre` is keyed by NAME not ID per the Navidrome contract (already documented on `SongSource::Genre` and `BatchItem::Genre`); it does not flow through the rating/star dispatch path because genres themselves are not starrable or ratable in the UI (`update/hotkeys/star_rating.rs:127` hard-codes `SlotListEntry::Parent(_) => None` for the genre case). If a future Navidrome version exposes genre rating, that's the moment to add the variant — not now.

---

## 2. Design choice: Option C (fresh enum + paired length-anchor)

The user posed three plausible alignments with the existing `BatchItem` enum:

- **Option A** — fresh marker enum `ItemKind { Album, Artist, Song, Playlist }`, no coupling to `BatchItem`.
- **Option B** — reuse `BatchItem`'s discriminant via `strum::EnumDiscriminants` or a hand-rolled `From<&BatchItem> for ItemKind`.
- **Option C** — Option A plus a `const _:` length-anchor that fails compilation if the variant counts of `ItemKind` and `BatchItem` drift apart.

**Recommendation: Option C.** Reasons:

1. **No new dep.** `CLAUDE.md` is explicit: *"rely on the existing workspace crates; discuss before adding new ones."* `strum` is not in the workspace today; pulling it in for one derive when a 2-line `const _: [(); …] = []` does the job is the wrong tradeoff.
2. **The `View::ALL` precedent at `61593bc` (§7 #2) lands the same shape.** Both directions of the const-anchor (`N - count` and `count - N`) are needed; this plan re-uses the exact pattern. An agent who has seen one knows how to read the other.
3. **`BatchItem::Genre(String)` is a true asymmetry.** `BatchItem` has 5 variants (`Song(Box<Song>)`, `Album`, `Artist`, `Genre`, `Playlist`); `ItemKind` has 4 (`Album`, `Artist`, `Song`, `Playlist`) — `Genre` is intentionally absent. So a literal `From<&BatchItem> for ItemKind` is partial (`Genre` → ?), and trying to encode that asymmetry in `From` either silently drops the genre case or panics. The const-anchor is **deliberately** length-mismatched (4 vs 5) and that's the documentation: it makes the asymmetry a typed fact the next agent reads in one place, not a comment in five.
4. **Payload nuance is fine.** `BatchItem::Song(Box<Song>)` carries the song; `ItemKind::Song` is payloadless. The const-anchor compares **lengths**, not payload shapes — so a future variant gain/loss on either side is caught regardless of payload.

The const-anchor reads:

```rust
// Length anchor: ItemKind has 4 variants; BatchItem has 5 (Genre is
// intentionally absent from ItemKind because Navidrome genres aren't
// starrable/ratable). Drift in either direction is a compile error.
//
// Adding a 5th ItemKind variant: bump both arrays to 5 and update
// every `match kind { … }` site (compiler will list them).
// Adding a 6th BatchItem variant: bump the BatchItem-side const to 6
// AND decide if it's starrable/ratable; if yes, add an ItemKind
// variant in lockstep.
const _: [(); 4 - ItemKind::ALL.len()] = [];
const _: [(); ItemKind::ALL.len() - 4] = [];
const _: [(); 5 - BATCH_ITEM_VARIANT_COUNT] = [];
const _: [(); BATCH_ITEM_VARIANT_COUNT - 5] = [];
```

Where `BATCH_ITEM_VARIANT_COUNT: usize = 5;` is a `pub const` declared at the top of `data/src/types/batch.rs` (line ~6, before the `enum BatchItem`). This is the only edit to `batch.rs`.

**Why Option A loses**: A `_ =>` arm on `ItemKind` is no improvement over today's `_ => Song` fallback once a 5th variant is added. The compile-error-on-drift is the whole point.

**Why Option B loses**: Strum is a 30-second `cargo add` and a docstring, but every dependency has a half-life of agent-confusion-about-which-derive-attribute-controls-this. Hand-rolling `From<&BatchItem> for ItemKind` is fine, but doing it requires either silently dropping `Genre` or returning `Option<ItemKind>` and dealing with the None at every call site — neither has a current caller (no code today goes `BatchItem → ItemKind`).

If a future caller actually needs the `BatchItem → ItemKind` mapping, the implementation is a 6-line `impl ItemKind { pub fn from_batch_item(item: &BatchItem) -> Option<Self> { … } }` added at that point. No upfront cost.

---

## 3. Architecture

### 3.1 `enum ItemKind` definition

**Location**: new module `data/src/types/item_kind.rs`, declared in `data/src/types/mod.rs` between `batch` and `song_source`.

```rust
//! Typed entity-kind discriminator for star/rating dispatch.
//!
//! Replaces the previous `item_type: &str` parameter that carried one of
//! `"album"`, `"artist"`, `"song"`. The drift-magic audit (§3) flagged the
//! string-keyed shape because `_ =>` fallbacks in `update/components.rs`
//! routed any unknown string (typo, future variant) silently to `Song`.
//!
//! Note: `Genre` is intentionally absent. Navidrome genres aren't
//! starrable or ratable today, and the rating-handler enumeration in
//! `src/update/hotkeys/star_rating.rs::get_center_item_info()` hard-codes
//! `SlotListEntry::Parent(_) => None` for the genre and playlist-parent
//! cases. `ItemKind::Playlist` is kept as a forward-compat slot since
//! the audit recommendation explicitly named it; today it's reachable
//! only via `BatchItem::Playlist` flattening, not via the star/rating
//! UI surface.
//!
//! Variant-count drift between `ItemKind` and `BatchItem` is locked by
//! `const _:` length anchors in this module — see the bottom of the file.

use crate::types::batch::BATCH_ITEM_VARIANT_COUNT;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ItemKind {
    Album,
    Artist,
    Song,
    Playlist,
}

impl ItemKind {
    /// Every `ItemKind` variant. Length-anchored — see the `const _:` lines.
    pub const ALL: &'static [ItemKind] = &[
        ItemKind::Album,
        ItemKind::Artist,
        ItemKind::Song,
        ItemKind::Playlist,
    ];

    /// Wire-format / log-label string. Stable: matches the prior
    /// `item_type: &str` literal at every site that fed the Subsonic
    /// API helpers (those helpers use the string only for error-message
    /// templating — the Subsonic `star`/`unstar`/`setRating` endpoints
    /// don't take a type discriminator on the wire).
    pub const fn api_str(self) -> &'static str {
        match self {
            ItemKind::Album => "album",
            ItemKind::Artist => "artist",
            ItemKind::Song => "song",
            ItemKind::Playlist => "playlist",
        }
    }
}

impl std::fmt::Display for ItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.api_str())
    }
}

// Length anchor: ItemKind has 4 variants; BatchItem has 5 (Genre is
// intentionally absent from ItemKind because Navidrome genres aren't
// starrable/ratable). Drift in either direction is a compile error.
const _: [(); 4 - ItemKind::ALL.len()] = [];
const _: [(); ItemKind::ALL.len() - 4] = [];
const _: [(); 5 - BATCH_ITEM_VARIANT_COUNT] = [];
const _: [(); BATCH_ITEM_VARIANT_COUNT - 5] = [];
```

Notes:

- `Copy` because the variants are payloadless — every site today passes the discriminator by value (`item_type: &'static str`) so `Copy` matches the existing ergonomics.
- `Hash` is added for forward-compat with potential `HashMap<ItemKind, _>` indexes (zero cost if unused).
- `pub const fn api_str` (not `pub fn`) so it composes in `const` contexts — keeps the door open for compile-time tables in follow-ups.
- The `Display` impl exists because `format!("Failed to {} {}", action, item_type)` patterns at `update/components.rs:775` use `item_type` as a `&str` today; a `Display` impl preserves the formatter syntax with no edit at the format-string site (`{} ` works on both `&str` and `ItemKind`). 

### 3.2 `BatchItem` const anchor

**Location**: top of `data/src/types/batch.rs`, before the `enum BatchItem` declaration.

```rust
use crate::types::song::Song;

/// Length anchor used by `ItemKind`'s drift checks. Update in lockstep
/// with the `enum BatchItem` variant count.
pub const BATCH_ITEM_VARIANT_COUNT: usize = 5;

#[derive(Debug, Clone)]
pub enum BatchItem {
    Song(Box<Song>),
    Album(String),
    Artist(String),
    Genre(String),
    Playlist(String),
}
// … rest unchanged
```

This is the only structural touch on `batch.rs`. No new derives, no payload edits.

### 3.3 Action-variant migration

The four per-view Action enums carry `&'static str` today. The migration converts them to `ItemKind`:

**Before (`src/views/albums/mod.rs:174-176`)**:
```rust
SetRating(String, &'static str, usize),
ToggleStar(String, &'static str, bool),
```

**After**:
```rust
SetRating(String, ItemKind, usize),
ToggleStar(String, ItemKind, bool),
```

Same three-tuple shape — only the second slot's type changes. The `&'static str` literals in callers (`"album"`, `"artist"`, `"song"`) become `ItemKind::Album`, `ItemKind::Artist`, `ItemKind::Song`. Every site is mechanically replaceable.

### 3.4 `starred_revert_message` / `rating_revert_message` exhaustive match

**Before (`src/update/components.rs:786-802`)**:
```rust
pub(crate) fn starred_revert_message(id: String, item_type: &str, starred: bool) -> Message {
    match item_type {
        "album" => Message::Hotkey(HotkeyMessage::AlbumStarredStatusUpdated(id, starred)),
        "artist" => Message::Hotkey(HotkeyMessage::ArtistStarredStatusUpdated(id, starred)),
        _ => Message::Hotkey(HotkeyMessage::SongStarredStatusUpdated(id, starred)),
    }
}

pub(crate) fn rating_revert_message(id: String, item_type: &str, rating: u32) -> Message {
    match item_type {
        "album" => Message::Hotkey(HotkeyMessage::AlbumRatingUpdated(id, rating)),
        "artist" => Message::Hotkey(HotkeyMessage::ArtistRatingUpdated(id, rating)),
        _ => Message::Hotkey(HotkeyMessage::SongRatingUpdated(id, rating)),
    }
}
```

**After**:
```rust
pub(crate) fn starred_revert_message(id: String, kind: ItemKind, starred: bool) -> Message {
    use HotkeyMessage::{
        AlbumStarredStatusUpdated, ArtistStarredStatusUpdated, SongStarredStatusUpdated,
    };
    Message::Hotkey(match kind {
        ItemKind::Album => AlbumStarredStatusUpdated(id, starred),
        ItemKind::Artist => ArtistStarredStatusUpdated(id, starred),
        // Playlist starring/unstarring isn't surfaced in the UI today
        // (playlist Parents return None from get_center_item_info, and
        // ClickToggleStar on a playlist Parent emits Action::None).
        // Until that lands, route Playlist through the Song handler so
        // a stray dispatch can't corrupt unrelated state — the handler
        // mutates only by-id matches, so it's a no-op for non-song ids.
        ItemKind::Song | ItemKind::Playlist => SongStarredStatusUpdated(id, starred),
    })
}

pub(crate) fn rating_revert_message(id: String, kind: ItemKind, rating: u32) -> Message {
    use HotkeyMessage::{AlbumRatingUpdated, ArtistRatingUpdated, SongRatingUpdated};
    Message::Hotkey(match kind {
        ItemKind::Album => AlbumRatingUpdated(id, rating),
        ItemKind::Artist => ArtistRatingUpdated(id, rating),
        ItemKind::Song | ItemKind::Playlist => SongRatingUpdated(id, rating),
    })
}
```

The change converts the silent `_ => Song` fallback into an explicit `ItemKind::Song | ItemKind::Playlist => …` arm. Adding a future `ItemKind::Genre` (or any other variant) is now a compiler error: the match becomes non-exhaustive and the build breaks. The decision **what to do** when starring a playlist gets made deliberately, in this file, in front of the dev — not silently in production.

The doc comment above each function gains one sentence: *"`ItemKind::Playlist` is collapsed into the Song handler today; revisit when playlist-level rating ships."*

### 3.5 `star_item_task` / `set_item_rating_task` signature

**Before (`src/update/components.rs:725`)**:
```rust
pub(crate) fn star_item_task(
    &self,
    id: String,
    item_type: &'static str,
    star: bool,
) -> Task<Message>
```

**After**:
```rust
pub(crate) fn star_item_task(
    &self,
    id: String,
    kind: ItemKind,
    star: bool,
) -> Task<Message>
```

Inside, `item_type` becomes `kind.api_str()` at the two `nokkvi_data::services::api::star::{star,unstar}_item` call sites and the two `format!`/`error!` formatters. `revert_id`'s sibling owned-string `revert_type: String` is gone — `kind` is `Copy`, so no clone-into-async-task gymnastics.

`set_item_rating_task` follows the same shape (`item_type: &str` → `kind: ItemKind`, drop the `revert_type: String` clone, replace `&revert_type` with `kind` at the inner `Self::rating_revert_message(revert_id, kind, current_rating)` call). The `nokkvi_data::services::api::rating::set_rating` call doesn't take a type label so no `api_str()` call is needed inside the closure.

### 3.6 `CenterItemInfo` field type

**Before (`src/update/hotkeys/star_rating.rs:9-16`)**:
```rust
pub(in crate::update) struct CenterItemInfo {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub starred: bool,
    pub rating: u32,
    pub item_type: &'static str,
}
```

**After**:
```rust
pub(in crate::update) struct CenterItemInfo {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub starred: bool,
    pub rating: u32,
    pub kind: ItemKind,
}
```

The 8 sites in `get_center_item_info` (lines 36, 50, 58, 73, 86, 94, 110, 125) flip from `item_type: "song"` to `kind: ItemKind::Song` etc. The two consumers (`handle_toggle_star` at `:144,156,158,160,183,191`; the rating handler at `:432,434,436,462`) drop the `to_string()` / `&revert_type` ceremony and pass `info.kind` (Copy) directly.

### 3.7 `view_header` `item_type` parameter — out of scope, stays `&str`

`src/widgets/view_header.rs:51` declares `item_type: &str`. It's used twice (lines 209, 211) in `format!("{filtered_count} of {total_count} {item_type}")`-style count strings, where the value is a **plural noun**: `"playlists"`, `"songs"`, `"albums"`, `"genres"`, `"stations"`, `"queue items"`, `"artists"` (verified at every call site in `src/views/{albums,artists,genres,playlists,queue,radios,songs,similar}/{view.rs,…}`).

This is **not the same `item_type` discriminator** the audit and `drift-magic.md` flag. It happens to share the parameter name. The plurals don't map to `ItemKind` (no `"stations"` variant; no `"queue items"`); they're display labels for header counts. Touching them would conflate two unrelated concerns and add a `to_plural()` helper that's unused outside view_header. **Stay `&str`.** Plan documents this as a deliberate non-pick.

### 3.8 `data/src/services/api/star.rs` — out of scope, stays `&str`

The `star_item / unstar_item / toggle_star` functions take `item_type: &str` solely to template error strings (`format!("Failed to star {item_type}")` at `:29`, `:51`). The Subsonic wire format is `id`-only — no type discriminator goes over HTTP. Migrating to `ItemKind` would couple the `data/services/api/` boundary to a UI-layer dispatch enum for log-string templating; the API surface is generic to any starrable id by design.

**Stay `&str` at the API boundary.** UI sites convert via `kind.api_str()` at the call site before invoking these helpers. This is a deliberate non-pick.

---

## 4. Per-call-site migration plan

Verified against `main @ 4edc0ec` (2026-05-09). Every line number below comes from `rg`/Read on this baseline, not from the audit.

### 4.1 Foundation (Lane A) — 1 file new + 1 file edit

| Site | Action |
|---|---|
| `data/src/types/item_kind.rs` (new) | Define `enum ItemKind`, `api_str()`, `Display`, `ALL`, const-anchors. |
| `data/src/types/mod.rs:7-8` | Add `pub mod item_kind;` between `batch` and `song_source`; export `pub use item_kind::ItemKind;`. |
| `data/src/types/batch.rs` (top) | Add `pub const BATCH_ITEM_VARIANT_COUNT: usize = 5;`. No structural change. |

Plus a co-landing `#[cfg(test)] mod tests` inside `item_kind.rs` covering the truth table for `api_str()` and the round-trip `ItemKind::ALL.iter().map(|k| k.api_str()).collect::<Vec<_>>() == ["album","artist","song","playlist"]`.

### 4.2 Action-variant + view-side dispatch (Lane B)

7 sites in 4 files. The `&'static str` slot in `Action::SetRating`/`Action::ToggleStar` becomes `ItemKind`; the literal-string callers in the per-view `update.rs` flip in lockstep.

| File:line | Variant | Edit |
|---|---|---|
| `src/views/albums/mod.rs:174` | `AlbumsAction::SetRating(String, &'static str, usize)` | `&'static str` → `ItemKind` |
| `src/views/albums/mod.rs:176` | `AlbumsAction::ToggleStar(String, &'static str, bool)` | same |
| `src/views/albums/update.rs:220,228` | `AlbumsAction::SetRating(.., "song", ..)` / `(.., "album", ..)` | `"song"` → `ItemKind::Song`, `"album"` → `ItemKind::Album` |
| `src/views/albums/update.rs:242,247-249` | `AlbumsAction::ToggleStar(.., "song", ..)` / `(.., "album", ..)` | same mapping |
| `src/views/artists/mod.rs:163` | `ArtistsAction::SetRating(String, &'static str, usize)` | type swap |
| `src/views/artists/mod.rs:165` | `ArtistsAction::ToggleStar(String, &'static str, bool)` | type swap |
| `src/views/artists/update.rs:177,185` | `SetRating(.., "album", ..)` / `(.., "artist", ..)` | literal swap |
| `src/views/artists/update.rs:195,199-202` | `ToggleStar(.., "album", ..)` / `(.., "artist", ..)` | literal swap |
| `src/views/genres/mod.rs:156` | `GenresAction::ToggleStar(String, &'static str, bool)` | type swap |
| `src/views/genres/update.rs:180` | `ToggleStar(.., "album", ..)` | `"album"` → `ItemKind::Album` |
| `src/views/playlists/mod.rs:169` | `PlaylistsAction::ToggleStar(String, &'static str, bool)` | type swap |
| `src/views/playlists/update.rs:175` | `ToggleStar(.., "song", ..)` | `"song"` → `ItemKind::Song` |

Doc-comments above each variant get the type-name update (`// (item_id, item_type, starred)` → `// (item_id, kind, starred)`).

### 4.3 Root dispatch + helpers (Lane C)

`src/update/components.rs` is the heart. 7 sites in this file change types; consumers update by ripple.

| File:line | Edit |
|---|---|
| `src/update/components.rs:725-782` | `star_item_task(id, item_type: &'static str, star)` → `(id, kind: ItemKind, star)`; inner sites pass `kind.api_str()` to API; `error!` and `debug!` formatters use `kind` (uses `Display` impl from §3.1). |
| `src/update/components.rs:786-791` | `starred_revert_message`: param `item_type: &str` → `kind: ItemKind`; body becomes the exhaustive match in §3.4. **Drops the `_ =>` wildcard.** |
| `src/update/components.rs:796-802` | `rating_revert_message`: same shape. **Drops the `_ =>` wildcard.** |
| `src/update/components.rs:806-862` | `set_item_rating_task(id, item_type: &str, …)` → `(id, kind: ItemKind, …)`; `revert_type: String` deleted; `&revert_type` → `kind`. |
| `src/update/components.rs:1031,1034` | `Self::starred_revert_message(.., "song", ..)` and `self.star_item_task(.., "song", ..)` → `ItemKind::Song`. |

`src/update/{albums,artists,genres,playlists,queue,similar,songs}.rs` consumer sites:

| File:line | Edit |
|---|---|
| `src/update/albums.rs:805,808,811` | `set_item_rating_task` / `starred_revert_message` / `star_item_task` calls — `item_type: &'static str` from the action variant flows through unchanged (`AlbumsAction::SetRating(item_id, item_type, ..)` is now `(item_id, kind, ..)`). |
| `src/update/artists.rs:595,598,601` | same shape |
| `src/update/genres.rs:481,484` | same shape |
| `src/update/playlists.rs:369,372` | same shape |
| `src/update/queue.rs:275,278,281` | hard-coded `"song"` → `ItemKind::Song` |
| `src/update/songs.rs:513,516,528` | hard-coded `"song"` → `ItemKind::Song` |
| `src/update/similar.rs:64,67` | hard-coded `"song"` → `ItemKind::Song` |

`src/update/artists.rs:562,565,571,574` (the dedicated `StarArtist`/`UnstarArtist` arms) use the literal `"artist"` — flip to `ItemKind::Artist`.

### 4.4 Hotkeys (Lane D)

`src/update/hotkeys/star_rating.rs` — `CenterItemInfo` field type swap (§3.6) + 8 literal sites + 2 consumer functions.

| File:line | Edit |
|---|---|
| `:15` | `pub item_type: &'static str` → `pub kind: ItemKind` |
| `:36,50,58,73,86,94,110,125` | `item_type: "song"` / `"album"` / `"artist"` → `kind: ItemKind::*` |
| `:144,156,158-160,183,191` | `info.item_type` → `info.kind`; remove the `to_string()` / `revert_type: String` ceremony (kind is `Copy`); inner `&item_type_owned` API call uses `kind.api_str()` |
| `:427,432,434,436,462` | same shape, rating handler |

Net: the file shrinks by ~6 LOC (the dropped owned-string clones for the async task closure).

### 4.5 Inventory totals

- **New files**: 1 (`data/src/types/item_kind.rs`).
- **Edited files**: 19 — 2 in `data/` (`types/{mod.rs,batch.rs}`) + 17 in `src/`:
  - `src/update/components.rs`
  - `src/update/hotkeys/star_rating.rs`
  - `src/update/{albums,artists,genres,playlists,queue,similar,songs}.rs`
  - `src/views/{albums,artists,genres,playlists}/mod.rs`
  - `src/views/{albums,artists,genres,playlists}/update.rs`
- **Wildcard arms eliminated**: 2 (`components.rs:790`, `:800`).
- **Deliberately unchanged**: `src/widgets/view_header.rs` (different concept — plural display labels), `data/src/services/api/{star,rating}.rs` (wire-format-stable boundary, `&str` is for error templating).

The audit cited **10 files** + the catch-all wildcards. Counted: 1 new + 19 edited = 20 files. The audit's count missed the per-view `mod.rs` Action declarations and didn't separate `views/playlists/mod.rs:169` from the other Action declarations. The 20-file figure above is the verified count at `4edc0ec`. Earlier draft summed to 14 by collapsing the per-view `{mod.rs,update.rs}` braces as one entry each instead of 8; the per-lane file lists in §5 were correct throughout.

---

## 5. Lane decomposition (fanout)

Four lanes, ordered. Lane A is the foundation (must merge first); Lanes B/C/D depend on Lane A's symbol but are pairwise-mergeable in any order; Lane E is the docs/audit-progress closer that runs last.

| Lane | Scope | Depends on | Effort | Files touched |
|---|---|---|---|---|
| **A** (foundation) | `enum ItemKind`, `api_str`, `Display`, `ALL`, const-anchors, `BATCH_ITEM_VARIANT_COUNT`, in-module unit tests | — | S | 3 (`data/src/types/item_kind.rs` new + `types/mod.rs` + `types/batch.rs`) |
| **B** (action variants + view dispatch) | 4 view Action enums + the per-view `update.rs` literal-string callers; updates the `Action::SetRating`/`Action::ToggleStar` second slot type only | A | M | 8 (`src/views/{albums,artists,genres,playlists}/{mod.rs,update.rs}`) |
| **C** (root helpers + per-view consumers) | `components.rs` helpers (3 fns) + 7 `update/*.rs` consumers; deletes the two `_ =>` wildcards | A | M | 8 (`src/update/components.rs` + `src/update/{albums,artists,genres,playlists,queue,similar,songs}.rs`) |
| **D** (hotkeys) | `star_rating.rs` — `CenterItemInfo` field swap + 8 literal sites + 2 handler bodies | A | S | 1 (`src/update/hotkeys/star_rating.rs`) |
| **E** (audit-progress + plan close) | `audit-progress.md` §7 row 5 + §3 row 6 + §4 row 2 to ✅ done with commit refs | B + C + D landed | S | 1 (`.agent/audit-progress.md`) |

**Lane B + Lane C must compile together.** If B lands first, C is broken (Action variants now carry `ItemKind` but root helpers expect `&str`); if C lands first, B is broken (helpers want `ItemKind` but Action variants still pass `&'static str`). The ordering protocol is:

- Land Lane A (atomic, compiles cleanly — no consumers yet).
- Land Lane B + Lane C **together** in a single fanout merge round. Both worktrees produce a sequence of commits; the merge order is B → C (or C → B) within the same PR-equivalent. Each individual commit need not compile, but the **merged tip** of `main` must compile after both lanes are integrated.
- Land Lane D after A (independent of B+C — `CenterItemInfo` is internal to `hotkeys/star_rating.rs` and feeds into the same `starred_revert_message` / `rating_revert_message` helpers; D's edits compile against A's symbol regardless of where B+C are).
- Land Lane E after B+C+D.

**Lesson from §7 #7 batch tidy-up (per CLAUDE.md spirit)**: don't over-slice. The 4-lane shape (vs. 7 tiny per-file lanes) is intentional. Per-commit compile under `cargo clippy --all-targets -- -D warnings` is preserved within each lane, and the cross-lane B↔C interlock is handled by sequencing the worktree merges, not by fragmenting the lanes further.

**Conflict zones**: Lane B and Lane C touch disjoint files (`src/views/` vs. `src/update/`); zero file overlap. Lane D is `src/update/hotkeys/star_rating.rs`, also disjoint from B/C. Lane A touches `data/src/types/`, disjoint from everything else.

---

## 6. Test plan

### 6.1 Today's coverage

`rg -n 'item_type|starred_revert|rating_revert' src/update/tests/` returns zero matches at baseline. The wildcard fallback is **completely untested**. That's the audit's whole point: the bug class is silent.

### 6.2 New tests landed in Lane A

In `data/src/types/item_kind.rs` (`#[cfg(test)] mod tests`):

```rust
#[test]
fn api_str_round_trip() {
    let pairs: Vec<(ItemKind, &str)> = ItemKind::ALL
        .iter()
        .map(|k| (*k, k.api_str()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            (ItemKind::Album, "album"),
            (ItemKind::Artist, "artist"),
            (ItemKind::Song, "song"),
            (ItemKind::Playlist, "playlist"),
        ]
    );
}

#[test]
fn display_matches_api_str() {
    for kind in ItemKind::ALL {
        assert_eq!(format!("{kind}"), kind.api_str());
    }
}

#[test]
fn all_has_no_duplicates() {
    let mut seen = std::collections::HashSet::new();
    for kind in ItemKind::ALL {
        assert!(seen.insert(*kind), "duplicate variant in ALL: {kind:?}");
    }
}
```

The const-anchors are themselves compile-time tests — if `BATCH_ITEM_VARIANT_COUNT` drifts from `BatchItem`'s real count, or `ItemKind::ALL.len()` drifts from `4`, the build fails. No runtime test required.

### 6.3 New tests landed in Lane C

In `src/update/tests/star_rating.rs` (new file if it doesn't exist; otherwise append). Per `.agent/rules/code-standards.md` §"Red-Green TDD Protocol", these go in the test module that pairs with the handler:

```rust
#[test]
fn starred_revert_message_routes_album_to_album_handler() {
    let msg = Nokkvi::starred_revert_message("a1".into(), ItemKind::Album, true);
    assert!(matches!(msg,
        Message::Hotkey(HotkeyMessage::AlbumStarredStatusUpdated(ref id, true))
            if id == "a1"
    ));
}

#[test]
fn starred_revert_message_routes_artist_to_artist_handler() {
    let msg = Nokkvi::starred_revert_message("ar1".into(), ItemKind::Artist, true);
    assert!(matches!(msg,
        Message::Hotkey(HotkeyMessage::ArtistStarredStatusUpdated(ref id, true))
            if id == "ar1"
    ));
}

#[test]
fn starred_revert_message_routes_song_to_song_handler() {
    let msg = Nokkvi::starred_revert_message("s1".into(), ItemKind::Song, false);
    assert!(matches!(msg,
        Message::Hotkey(HotkeyMessage::SongStarredStatusUpdated(ref id, false))
            if id == "s1"
    ));
}

#[test]
fn starred_revert_message_routes_playlist_through_song_handler_for_now() {
    // Until playlist-level rating ships, ItemKind::Playlist collapses
    // into the Song handler. This test pins that decision so a future
    // change is deliberate, not silent.
    let msg = Nokkvi::starred_revert_message("p1".into(), ItemKind::Playlist, true);
    assert!(matches!(msg,
        Message::Hotkey(HotkeyMessage::SongStarredStatusUpdated(ref id, true))
            if id == "p1"
    ));
}
```

Plus the four mirror tests for `rating_revert_message` (Album/Artist/Song/Playlist).

This is **8 new tests** — the table the audit asks for (`drift-magic.md` line 161: "Replace every `&str` parameter and the catch-all `_ =>` fallback in `starred_revert_message` / `rating_revert_message` with exhaustive matches"). The catch-all is gone; the table is now pinned by tests + by the compiler's exhaustiveness checker.

### 6.4 Existing tests

Lane B/C/D rename `item_type` → `kind` everywhere it appears in source. The existing test files don't reference `item_type` (verified: `rg item_type src/update/tests/` returns zero hits). No test edits needed for the rename itself; the `Action::SetRating(.., ItemKind::Album, ..)` / `Action::ToggleStar(.., ItemKind::Song, ..)` constructors compile under the new types as long as the test sites use the new variant names. If any existing test constructs an `Action::SetRating` or `Action::ToggleStar` directly, it gets the literal swap (`"song"` → `ItemKind::Song`); audit notes none today, but verify with `rg 'AlbumsAction::SetRating|ArtistsAction::SetRating|GenresAction::ToggleStar|PlaylistsAction::ToggleStar' src/` during Lane B implementation.

---

## 7. Verification (every lane)

Run after each commit slice:

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

All four must pass before pushing the slice. SVG paths aren't touched (no `assets/icons/` edits anywhere in this plan), so the `cargo test --bin nokkvi -- embedded_svg` gate is a no-op — running it is fine but no failures are expected.

**Lane A specific check**:
- `grep -c 'pub enum ItemKind' data/src/types/item_kind.rs` should equal `1`.
- `grep -c '_ =>' data/src/types/item_kind.rs` should equal `0` (no wildcards in the new module).
- `cargo test -p nokkvi-data item_kind::` should list 3 tests passing (`api_str_round_trip`, `display_matches_api_str`, `all_has_no_duplicates`).

**Lane B specific check**:
- `rg "&'static str" src/views/{albums,artists,genres,playlists}/mod.rs | grep -E 'SetRating|ToggleStar'` should return zero hits.
- `rg '\"album\"|\"artist\"|\"song\"' src/views/{albums,artists,genres,playlists}/update.rs | grep -E 'SetRating|ToggleStar'` should return zero hits (every literal swapped to `ItemKind::*`).

**Lane C specific check**:
- `rg '_ =>' src/update/components.rs | wc -l` should equal `0` for the two prior wildcard sites (others may exist in unrelated arms — confirm by reviewing the full file).
- `rg 'item_type' src/update/components.rs src/update/{albums,artists,genres,playlists,queue,similar,songs}.rs` should return zero hits (every parameter renamed to `kind`).
- `cargo test -p nokkvi star_rating` (or whatever the test module is named) should list 8+ tests passing for the revert-message routing.

**Lane D specific check**:
- `rg 'item_type' src/update/hotkeys/` should return zero hits.
- `rg 'revert_type' src/update/hotkeys/` should return zero hits (the owned-string clone is gone — `ItemKind: Copy`).
- `wc -l src/update/hotkeys/star_rating.rs` should report a smaller LOC than baseline (estimated ~6 LOC reduction from the dropped `to_string()` + `let revert_type =` ceremony).

---

## 8. What this plan does NOT do

- **No new dependency.** Pure std + existing workspace crates. No `strum`, no `derive_more`, no proc macros.
- **No edits to `data/src/services/api/{star,rating}.rs`.** The wire format is unchanged; the `item_type: &str` parameter on the API helpers is for error-message templating only and stays `&str`. Call sites convert via `kind.api_str()` at the boundary.
- **No edits to `src/widgets/view_header.rs`.** Its `item_type: &str` is a plural display label (`"songs"`, `"playlists"`, `"stations"`), not the entity discriminator. Touching it would conflate two unrelated concepts.
- **No `Genre` variant.** Navidrome genres aren't starrable/ratable and the `get_center_item_info` enumeration explicitly returns `None` for `SlotListEntry::Parent` on genres + playlists. The const-anchor encodes the asymmetry as a typed fact (4 vs 5 variants between `ItemKind` and `BatchItem`).
- **No `From<&BatchItem> for ItemKind` impl.** No caller today; YAGNI. The const-anchor catches drift; the impl can be added later if a caller materializes.
- **No fold of the `dry-handlers.md` #5 "ToggleStar with optimistic revert" pattern (`toggle_star_with_revert_task`).** That's a separate audit row (`_SYNTHESIS.md` §3 row 13 / §7 unranked). Migrating to `ItemKind` doesn't block it; the helper can land on top of `ItemKind` cleanly afterwards.
- **No fold of the `dry-handlers.md` #6 "Hotkey star/rating boilerplate routing" item.** The audit's #6 fix is about routing `auth_vm.get_client → http_client → server_url → subsonic_credential → api::star/rating` through helper methods on `Nokkvi` rather than re-deriving the chain at every call site. That's orthogonal to the `ItemKind` migration: the helper-routing fix changes **how** the API call is made; this plan only changes **what type carries the entity discriminator**. The two refactors compose; this plan doesn't block #6.
- **No reformatting outside touched files.**
- **No drive-by docstring rewrites unrelated to the migration.**
- **No changes to `.agent/rules/` files.** Only `.agent/audit-progress.md` (Lane E) records the closure.
- **No CI grep-test added** (per `pending-expand-dedup.md` §6 precedent — out of scope here).

---

## 9. Closing — `.agent/audit-progress.md` updates (Lane E)

After Lanes A+B+C+D land, Lane E flips three rows in `.agent/audit-progress.md` and lands a single docs commit. Suggested commit body modeled on `4edc0ec`:

```
docs(audit): close §7 #5 / Drift #2 after ItemKind migration lands

Replaces the "❌ open" status on §7 row 5, §3 row 6, and §4 row 2
with explicit commit refs for the four-lane fanout from
.agent/plans/itemkind-migration.md (2026-05-09):

- Lane A (foundation): <commit-A> — new data/src/types/item_kind.rs
  with `enum ItemKind { Album, Artist, Song, Playlist }`, `api_str()`,
  `Display`, `ALL`, paired const _: length anchors against
  BATCH_ITEM_VARIANT_COUNT in batch.rs; 3 in-module tests for the
  truth table.
- Lane B (action variants + view dispatch): <commits-B> — 4 view
  Action enums (SetRating/ToggleStar second slot) and 7 per-view
  update.rs literal-string callers flipped from `&'static str` to
  ItemKind. 14 literal `"album"`/`"artist"`/`"song"` strings replaced
  with ItemKind::* across src/views/{albums,artists,genres,playlists}.
- Lane C (root helpers + per-view consumers): <commits-C> — the two
  `_ => …Song` wildcards in components.rs:790,800 are gone;
  starred_revert_message / rating_revert_message are exhaustive
  matches on ItemKind with explicit `Song | Playlist => …` arms.
  star_item_task / set_item_rating_task signatures take ItemKind
  (Copy) and drop the owned-string clone for the async task closure.
  7 update/*.rs consumer sites flipped.
- Lane D (hotkeys): <commit-D> — CenterItemInfo.item_type:
  &'static str → kind: ItemKind; 8 literal sites + 2 handler bodies
  in src/update/hotkeys/star_rating.rs migrated; ~6 LOC reduction
  from dropped revert_type: String clones.

Bug class closed: typing `"alubm"` no longer compiles. Adding a
future ItemKind variant fails the build at every match site.
Adding a 6th BatchItem variant fails the const-anchor in
data/src/types/item_kind.rs.

8 new revert-message routing tests pin the truth table for
{Album,Artist,Song,Playlist} × {starred,rating} -> Hotkey* variants.
The Playlist arm explicitly collapses into the Song handler today
with a comment noting the revisit-when-playlist-level-rating-ships
trigger.

§3 row 6 ("Hotkey star/rating boilerplate") and §4 row 2
("item_type: &str carrying entity kind") are flipped to ✅ done in
the same commit since they reference the same migration.
```

The three table-row edits in `.agent/audit-progress.md`:

- **§7 row 5** — flip status `❌ open` → `✅ done`; replace evidence column with commit refs; bold the rank like other completed rows.
- **§3 row 6** — flip status `❓ stale path` → `✅ done`; evidence note: "Same as §7 #5. The audit-progress §7 #4 'Migrate `hotkeys/star_rating.rs`' note about the stale path is unrelated — that item refers to inline-rebuild routing, not the ItemKind migration."
- **§4 row 2** — flip status `❌ open` → `✅ done`; evidence: "Same as §7 #5."

---

## Fanout Prompts

### lane-a-foundation

worktree: ~/nokkvi-itemkind-a
branch: refactor/itemkind-foundation
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane A of the ItemKind migration plan — introduce the `enum ItemKind` foundation in the data crate.

Plan doc: /home/foogs/nokkvi/.agent/plans/itemkind-migration.md (sections 2, 3.1, 3.2, 4.1, 6.2).

Working directory: ~/nokkvi-itemkind-a (this worktree). Branch: refactor/itemkind-foundation. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` should show `4edc0ec` or a descendant on `main`.
- `rg -n 'enum ItemKind|item_kind' data/src src/` must return zero matches outside docs/comments — no `ItemKind` exists yet anywhere.
- `wc -l data/src/types/batch.rs` should report ~31 lines.

If any of those drift, STOP and ask before continuing.

### 2. Add the BatchItem length anchor

Edit `data/src/types/batch.rs`. Above the `enum BatchItem` declaration (currently line 7), add:

```rust
/// Length anchor used by `ItemKind`'s drift checks. Update in lockstep
/// with the `enum BatchItem` variant count.
pub const BATCH_ITEM_VARIANT_COUNT: usize = 5;
```

No other changes to `batch.rs`. No new derives, no payload edits.

### 3. Create data/src/types/item_kind.rs

Create the file exactly per plan §3.1 (the verbatim block including module-level doc comment, the enum, the impl with `ALL` + `api_str()`, the `Display` impl, and the four `const _:` length anchors against `BATCH_ITEM_VARIANT_COUNT`).

Then add a `#[cfg(test)] mod tests` at the bottom with the three tests from plan §6.2:
- `api_str_round_trip` — pins the truth table.
- `display_matches_api_str` — pins the Display impl.
- `all_has_no_duplicates` — sanity check.

### 4. Wire the module into types/mod.rs

Edit `data/src/types/mod.rs`. Between line 7 (`pub mod batch;`) and line 8 (`pub mod collage_artwork;`), insert:

```rust
pub mod item_kind;
```

Then in the `pub use` block (around line 41-42, where `pub use mode_toggle::ModeToggleEffect;` and `pub use song_source::SongSource;` live), add:

```rust
pub use item_kind::ItemKind;
```

Keep the alphabetical-ish order — slot it before `mode_toggle::*`.

### 5. Verify in this order, fixing any failure before continuing

```
cargo build
cargo test -p nokkvi-data item_kind::
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

The const-anchors are compile-time tests; if `BATCH_ITEM_VARIANT_COUNT` is wrong or `ItemKind::ALL.len()` doesn't match `4`, `cargo build` fails with a const-eval error pointing at the anchor lines.

### 6. Commit

Use the conventional-commits format from `~/nokkvi/CLAUDE.md`. Suggested message:

    refactor(data): add ItemKind enum + BatchItem length anchor

    Foundation for the §7 #5 / Drift #2 migration that replaces
    `item_type: &str` carrying "album"/"artist"/"song" with a typed
    enum across 14 files.

    The const _: pairs at the bottom of types/item_kind.rs anchor
    ItemKind's variant count against BATCH_ITEM_VARIANT_COUNT in
    batch.rs — drift in either direction is a compile error,
    matching the View::ALL precedent at 61593bc.

    ItemKind::Genre is intentionally absent. Navidrome genres aren't
    starrable/ratable today (get_center_item_info returns None for
    SlotListEntry::Parent on genres + playlists). The asymmetry is
    documented on the enum and pinned by the const-anchor (4 vs 5).

    Three in-module tests pin the api_str() truth table, the Display
    impl agreement, and ALL having no duplicates.

    Part of `.agent/plans/itemkind-migration.md` (Lane A).

Skip the Co-Authored-By trailer per global instructions.

### 7. Reporting

End with a short summary: which commit, which files changed, line counts (new file LOC + the 1-line edits in batch.rs and types/mod.rs).

## What NOT to touch

- `src/` (the UI crate). All UI-side migrations are Lanes B/C/D.
- Any other `data/src/types/*.rs` file beyond `mod.rs` + `batch.rs`.
- `.agent/` (Lane E's territory).
- Any other audit item.

## If blocked

- If `BATCH_ITEM_VARIANT_COUNT = 5` doesn't match `BatchItem`'s actual variant count (count `pub enum BatchItem { … }` arms in `batch.rs`): adjust the constant to match, but STOP and report — the plan needs revising if `BatchItem` has a different shape than expected.
- If `cargo test` fails on an unrelated baseline test: stop, report, do not proceed.
- If clippy flags the const-anchor pattern (e.g., `clippy::let_unit_value` on `const _: [(); …] = [];`): adjust minimally; do not paper over with `#[allow]` unless the precedent at `src/main.rs:78` does the same (verify by reading that file before adding any `#[allow]`).
````

### lane-b-action-variants

worktree: ~/nokkvi-itemkind-b
branch: refactor/itemkind-action-variants
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane B of the ItemKind migration plan — flip the `Action::SetRating`/`Action::ToggleStar` second slot from `&'static str` to `ItemKind` in the four view crates that carry it, and convert all literal-string callers in their per-view `update.rs` files.

Plan doc: /home/foogs/nokkvi/.agent/plans/itemkind-migration.md (sections 3.3, 4.2, 5).

Working directory: ~/nokkvi-itemkind-b (this worktree). Branch: refactor/itemkind-action-variants. The worktree is already created — do NOT run `git worktree add`.

## Important — cross-lane dependency

Lane B and Lane C must compile together. After this lane is done in isolation, `cargo build` will FAIL because `src/update/{albums,artists,genres,playlists}.rs` still match the old `Action::ToggleStar(_, item_type, _)` shape against the new `ItemKind`-typed variant. That's expected — Lane C handles the consumer side. Push your branch when each commit is locally clean (`cargo +nightly fmt --all -- --check` and `cargo clippy --no-deps -- -D warnings` on the touched files); the cross-lane build correctness is verified after the Lane B + Lane C merge round.

If you need a clean local build during development, you may temporarily apply a one-line patch to `src/update/components.rs` and `src/update/{albums,artists,genres,playlists,queue,similar,songs}.rs` to convert the receiver side — but **revert that patch before commit**. Lane C owns those files.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` should show Lane A's commit on top of `4edc0ec`. (You're branching off post-Lane-A; the `enum ItemKind` symbol must be importable.)
- `rg -n 'use nokkvi_data::types::ItemKind' src/` should return zero hits initially (you're the first consumer).
- `rg -n "&'static str" src/views/{albums,artists,genres,playlists}/mod.rs | rg -E 'SetRating|ToggleStar'` should list these 6 hits:
  - `src/views/albums/mod.rs:174` (SetRating)
  - `src/views/albums/mod.rs:176` (ToggleStar)
  - `src/views/artists/mod.rs:163` (SetRating)
  - `src/views/artists/mod.rs:165` (ToggleStar)
  - `src/views/genres/mod.rs:156` (ToggleStar)
  - `src/views/playlists/mod.rs:169` (ToggleStar)

If you see fewer or more, STOP and report — the plan's call-site enumeration is wrong.

### 2. Update the four Action variants

For each of the 6 hits above, swap `&'static str` to `ItemKind`. The enclosing tuple shape stays exactly the same (3-tuple for SetRating/ToggleStar). Add `use nokkvi_data::types::ItemKind;` at the top of each `mod.rs` if not already present.

Doc-comment update: where the comment reads `// (item_id, item_type, …)` or `// item_type "album"|"song"`, change to `// (item_id, kind, …)`.

### 3. Update the literal-string callers in update.rs

For each per-view `update.rs`, swap:
- `"album"` → `ItemKind::Album`
- `"artist"` → `ItemKind::Artist`
- `"song"` → `ItemKind::Song`
- `"playlist"` → `ItemKind::Playlist` (none today, but if you find one, flag it)

Verified literal sites (line numbers from baseline `4edc0ec`):
- `src/views/albums/update.rs:220, 228` (SetRating)
- `src/views/albums/update.rs:242, 247-249` (ToggleStar)
- `src/views/artists/update.rs:177, 185` (SetRating)
- `src/views/artists/update.rs:195, 199-202` (ToggleStar)
- `src/views/genres/update.rs:180` (ToggleStar)
- `src/views/playlists/update.rs:175` (ToggleStar)

Add `use nokkvi_data::types::ItemKind;` at the top of each `update.rs` if not already present.

### 4. Verify the per-file fmt + clippy

For each touched file, run:
```
cargo +nightly fmt --check -- src/views/albums/mod.rs src/views/albums/update.rs ...
```
(or just `cargo +nightly fmt --all -- --check` if simpler).

Full `cargo build` will fail at this point — that's expected per the cross-lane dependency note above. Skip `cargo test` for the same reason.

### 5. Commit

One or two commits, conventional format. Suggested split:

Commit 1 — Action variant types only:
    refactor(views): flip SetRating/ToggleStar Action slot to ItemKind

    Action::SetRating and Action::ToggleStar in albums/artists/genres/
    playlists view crates carried `&'static str` for the entity-kind
    discriminator. Per `.agent/plans/itemkind-migration.md` Lane B,
    swap to nokkvi_data::types::ItemKind so a typo no longer compiles
    and a future variant gain breaks the build at every match site.

    Lane C (root helpers + per-view consumers) lands separately and
    must merge in the same round — `cargo build` fails on this commit
    in isolation by design.

    Part of `.agent/plans/itemkind-migration.md` (Lane B, action types).

Commit 2 — literal-string callers:
    refactor(views): flip `"album"`/`"artist"`/`"song"` literals to ItemKind

    Updates the per-view update.rs callers that construct
    Action::SetRating / Action::ToggleStar to pass ItemKind::* instead
    of string literals. 14 literal swaps across 4 update.rs files.

    Part of `.agent/plans/itemkind-migration.md` (Lane B, literals).

Skip the Co-Authored-By trailer per global instructions.

### 6. Reporting

End with a short summary: which commit(s), which files changed, line counts. Note explicitly that `cargo build` is expected to fail until Lane C merges — that's not a regression.

## What NOT to touch

- `src/update/` (Lane C's territory).
- `src/update/hotkeys/` (Lane D's territory).
- `data/` (Lane A's territory; already landed).
- `.agent/` (Lane E's territory).
- Any other view file (`src/views/{songs,queue,similar,radios}/*` — those views' Action enums use `&'static str` only when the kind is hard-coded, so they don't carry the parameter).

## If blocked

- If a literal `"playlist"` appears in any of the 4 update.rs files: flag it in your report. The plan didn't list any, but if one exists, swap to `ItemKind::Playlist` and note the surprise.
- If a view crate's `mod.rs` has more than the 6 hits enumerated above (e.g., a 5th `Action` variant that carries `&'static str`): list it and ask before continuing — the plan's scope might need expanding.
- If `cargo +nightly fmt` mangles the doc comments: the rustfmt.toml uses unstable features (`group_imports`, `imports_granularity`); just trust the formatter and re-stage.
````

### lane-c-helpers-and-consumers

worktree: ~/nokkvi-itemkind-c
branch: refactor/itemkind-helpers
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane C of the ItemKind migration plan — flip `starred_revert_message` / `rating_revert_message` to exhaustive `ItemKind` matches (deleting the two `_ => …Song` wildcards), update `star_item_task` / `set_item_rating_task` signatures, and migrate the 7 per-view consumer sites.

Plan doc: /home/foogs/nokkvi/.agent/plans/itemkind-migration.md (sections 3.4, 3.5, 4.3, 6.3).

Working directory: ~/nokkvi-itemkind-c (this worktree). Branch: refactor/itemkind-helpers. The worktree is already created — do NOT run `git worktree add`.

## Important — cross-lane dependency

Lane B and Lane C must compile together. Lane B updates the Action variant types in `src/views/{albums,artists,genres,playlists}/{mod,update}.rs`. Lane C updates the helpers in `src/update/components.rs` plus 7 consumer files in `src/update/`. After Lane B merges into your starting point and Lane C is done, the merged tip compiles cleanly. **You are starting from the post-Lane-A `main`** — Lane B's commits are NOT in your worktree yet. The `Action::SetRating`/`Action::ToggleStar` callers in `update/{albums,artists,genres,playlists}.rs` will need their incoming-arg types to match the new ItemKind shape (those Action variants come from Lane B); the consumer sites you edit destructure them by position, so as long as the destructured slot is renamed `kind` everywhere and used as `ItemKind`, the merge works after Lane B lands.

If you need a clean local build during development, you may temporarily apply a stub patch to `src/views/{albums,artists,genres,playlists}/mod.rs` mirroring Lane B's edits — but **revert before commit**. Lane B owns those files.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` should show Lane A's commit on top of `4edc0ec`.
- `rg -n '_ =>' src/update/components.rs | head -5` — there should be hits at line 790 and 800 (the Song fallback wildcards). Those are your target deletions.
- `wc -l src/update/components.rs` — note the LOC for the post-edit reporting.

### 2. Migrate `starred_revert_message` (line 786-791)

Per plan §3.4. The new body is:

```rust
pub(crate) fn starred_revert_message(id: String, kind: ItemKind, starred: bool) -> Message {
    use HotkeyMessage::{
        AlbumStarredStatusUpdated, ArtistStarredStatusUpdated, SongStarredStatusUpdated,
    };
    Message::Hotkey(match kind {
        ItemKind::Album => AlbumStarredStatusUpdated(id, starred),
        ItemKind::Artist => ArtistStarredStatusUpdated(id, starred),
        // Playlist starring/unstarring isn't surfaced in the UI today
        // (playlist Parents return None from get_center_item_info, and
        // ClickToggleStar on a playlist Parent emits Action::None).
        // Until that lands, route Playlist through the Song handler so
        // a stray dispatch can't corrupt unrelated state — the handler
        // mutates only by-id matches, so it's a no-op for non-song ids.
        ItemKind::Song | ItemKind::Playlist => SongStarredStatusUpdated(id, starred),
    })
}
```

Note: the explicit `ItemKind::Song | ItemKind::Playlist` arm replaces the silent `_ => …Song` wildcard. This is the headline behavioral change.

### 3. Migrate `rating_revert_message` (line 796-802)

Same shape:

```rust
pub(crate) fn rating_revert_message(id: String, kind: ItemKind, rating: u32) -> Message {
    use HotkeyMessage::{AlbumRatingUpdated, ArtistRatingUpdated, SongRatingUpdated};
    Message::Hotkey(match kind {
        ItemKind::Album => AlbumRatingUpdated(id, rating),
        ItemKind::Artist => ArtistRatingUpdated(id, rating),
        ItemKind::Song | ItemKind::Playlist => SongRatingUpdated(id, rating),
    })
}
```

### 4. Migrate `star_item_task` (line 725-782)

Per plan §3.5. Signature swap:
- Param `item_type: &'static str` → `kind: ItemKind`.
- Inner `nokkvi_data::services::api::star::{star,unstar}_item(.., &id, item_type)` → `(.., &id, kind.api_str())`.
- The `error!` and `debug!` formatters that interpolate `item_type` use `kind` directly (relies on the `Display` impl from Lane A).
- Self::starred_revert_message at line 777: pass `kind` (Copy), drop the prior `item_type` rebinding.

### 5. Migrate `set_item_rating_task` (line 806-862)

- Param `item_type: &str` → `kind: ItemKind`.
- Delete the `let revert_type = item_type.to_string();` line (kind is Copy; no clone needed).
- Inner `Self::rating_revert_message(.., item_type, ..)` and `(.., &revert_type, ..)` → both pass `kind`.
- The Subsonic `set_rating` API call doesn't take a type discriminator, so no `api_str()` is needed inside the closure.

### 6. Migrate the strip-context-menu site (line 1031, 1034)

`Self::starred_revert_message(song_id.clone(), "song", new_starred)` → `(.., ItemKind::Song, ..)`.
`self.star_item_task(song_id, "song", new_starred)` → `(.., ItemKind::Song, ..)`.

### 7. Migrate the 7 per-view consumers

Each of these files calls one or more of `set_item_rating_task` / `starred_revert_message` / `star_item_task`. Update both the destructure slot name (`item_type` → `kind`) when receiving from an `Action` variant, and any hard-coded literal `"song"`/`"album"`/`"artist"` strings:

- `src/update/albums.rs:805, 808, 811` — destructure-from-Action, kind flows through.
- `src/update/artists.rs:562, 565, 571, 574, 595, 598, 601` — note the `StarArtist`/`UnstarArtist` arms at :562-575 use literal `"artist"` strings; flip to `ItemKind::Artist`.
- `src/update/genres.rs:481, 484` — destructure-from-Action.
- `src/update/playlists.rs:369, 372` — destructure-from-Action.
- `src/update/queue.rs:275, 278, 281` — hard-coded `"song"` → `ItemKind::Song`.
- `src/update/similar.rs:64, 67` — hard-coded `"song"` → `ItemKind::Song`.
- `src/update/songs.rs:513, 516, 528` — hard-coded `"song"` → `ItemKind::Song`.

Add `use nokkvi_data::types::ItemKind;` at the top of each touched file if not already present.

### 8. Add the revert-message routing tests

Find the test module that pairs with components.rs handlers. Per `.agent/rules/code-standards.md` §"Red-Green TDD Protocol", new handler tests live in `src/update/tests.rs` or a co-located `tests_*.rs`. Locate the existing test module (likely `src/update/tests/` directory or `src/update/tests.rs` file) by running:

```
ls src/update/tests/ src/update/tests.rs 2>/dev/null
```

Add 8 new tests per plan §6.3:
- `starred_revert_message_routes_album_to_album_handler`
- `starred_revert_message_routes_artist_to_artist_handler`
- `starred_revert_message_routes_song_to_song_handler`
- `starred_revert_message_routes_playlist_through_song_handler_for_now`
- 4 mirror tests for `rating_revert_message`.

If a `star_rating.rs` test file already exists in `src/update/tests/`, append there. Otherwise, create `src/update/tests/star_rating.rs` and declare it in the parent `mod.rs`.

### 9. Verify (after both Lane B and your Lane C land in the merge round; you can't run a clean build in isolation)

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

The 8 new tests should pass; the existing test count should not drop.

### 10. Commit

Suggested split (3 commits, all conventional format):

Commit 1 — exhaustive matches (the wildcard delete):
    refactor(update): make starred/rating revert dispatch exhaustive on ItemKind

    starred_revert_message and rating_revert_message in
    src/update/components.rs each had a `_ => …Song` wildcard fallback
    that silently routed any unknown item_type string (typo, future
    variant) to the Song handler. Per `.agent/plans/itemkind-migration.md`
    (Drift #2 fix), the param is now `kind: ItemKind` and the match
    is exhaustive: `ItemKind::Song | ItemKind::Playlist => …`. A future
    `ItemKind::Genre` (or any other variant gain) is a compile error.

    The Playlist arm explicitly collapses into the Song handler today
    with a comment noting the revisit-when-playlist-rating-ships
    trigger. This is the headline bug-class fix for §7 #5 / Drift #2.

    Part of `.agent/plans/itemkind-migration.md` (Lane C, helpers).

Commit 2 — task-helper signatures + strip menu:
    refactor(update): flip star_item_task / set_item_rating_task to ItemKind

    Drops the owned `revert_type: String` clone (ItemKind is Copy)
    and uses kind.api_str() at the API boundary. The strip-context
    site at components.rs:1031,1034 flips two literal "song" strings.

    Part of `.agent/plans/itemkind-migration.md` (Lane C, task helpers).

Commit 3 — 7 per-view consumers + 8 new tests:
    refactor(update): migrate 7 per-view consumers + pin revert routing

    src/update/{albums,artists,genres,playlists,queue,similar,songs}.rs
    consumer sites flipped to ItemKind. The artists.rs StarArtist /
    UnstarArtist arms flip their literal "artist" strings.

    8 new tests in src/update/tests/star_rating.rs pin the truth
    table for {Album,Artist,Song,Playlist} × {starred,rating} ->
    Hotkey* variants. The Playlist→Song collapse is now compiler-
    pinned AND test-pinned.

    Part of `.agent/plans/itemkind-migration.md` (Lane C, consumers).

Skip the Co-Authored-By trailer per global instructions.

### 11. Reporting

End with: commit hashes, files changed counts, LOC delta on components.rs (expect minor reduction — the format!("{}") interpolation flows through Display). Confirm both wildcard deletions explicitly: `rg '_ =>' src/update/components.rs` should return zero hits in the two prior wildcard arms (other unrelated arms may still exist; confirm by checking line numbers).

## What NOT to touch

- `src/update/hotkeys/` (Lane D's territory).
- `src/views/` (Lane B's territory).
- `data/` (Lane A's territory).
- `src/widgets/view_header.rs` — its `item_type: &str` is a plural display label, NOT the entity discriminator. Leave it alone.
- `data/src/services/api/star.rs` and `data/src/services/api/rating.rs` — wire-format-stable boundary, item_type is for error templating, leave it `&str`. Call sites convert via `kind.api_str()`.

## If blocked

- If a literal `"album"`/`"artist"`/`"song"` appears in `src/update/*.rs` outside the enumerated sites: list it and ask. The plan enumerated 7 consumer files; surprises are flag-worthy.
- If `set_item_rating_task` or `star_item_task` has more than 1 caller per consumer file: that's expected (some files use both). Migrate each independently.
- If clippy flags the new exhaustive match (e.g., `clippy::needless_match`): adjust minimally; the explicit `Song | Playlist` arm is intentional — don't suggest collapsing it back to a wildcard.
````

### lane-d-hotkeys

worktree: ~/nokkvi-itemkind-d
branch: refactor/itemkind-hotkeys
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane D of the ItemKind migration plan — flip `CenterItemInfo.item_type: &'static str` to `kind: ItemKind`, migrate 8 literal sites in `get_center_item_info`, and rewrite `handle_toggle_star` + the rating handler to drop the `revert_type: String` clone (ItemKind is Copy).

Plan doc: /home/foogs/nokkvi/.agent/plans/itemkind-migration.md (sections 3.6, 4.4).

Working directory: ~/nokkvi-itemkind-d (this worktree). Branch: refactor/itemkind-hotkeys. The worktree is already created — do NOT run `git worktree add`.

## What to do

### 1. Verify baseline

- `git log -1 --oneline` should show Lane A's commit on top of `4edc0ec` (Lane A is the only required dependency for D — D doesn't need B/C to compile because it only uses ItemKind from the data crate).
- `rg -n 'item_type' src/update/hotkeys/star_rating.rs | wc -l` should report 14 hits (1 struct field + 8 literals + 5 consumer references).

### 2. Migrate the CenterItemInfo struct field (line 9-16)

```rust
pub(in crate::update) struct CenterItemInfo {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub starred: bool,
    pub rating: u32,
    pub kind: ItemKind,
}
```

Add `use nokkvi_data::types::ItemKind;` at the top of the file.

### 3. Migrate the 8 literal sites in get_center_item_info

Lines 36, 50, 58, 73, 86, 94, 110, 125:
- `item_type: "song"` → `kind: ItemKind::Song`
- `item_type: "album"` → `kind: ItemKind::Album`
- `item_type: "artist"` → `kind: ItemKind::Artist`

### 4. Migrate handle_toggle_star (lines 133-223)

- `info.item_type` (line 144) → `info.kind` — `Display` impl from Lane A handles formatter interpolation.
- `Self::starred_revert_message(info.id.clone(), info.item_type, new_starred)` (line 156) → `(.., info.kind, ..)`.
- DELETE `let item_type_owned = info.item_type.to_string();` (line 158).
- DELETE `let revert_type = info.item_type.to_string();` (line 160).
- The async closure inner `&item_type_owned` (line 183) → `info.kind.api_str()`. Since `info` is consumed by the closure move, you'll need to either:
  - Move only the parts you need: `let toggle_kind = info.kind;` early; pass `toggle_kind.api_str()` inside the closure.
  - Or restructure the captures so `kind` is captured directly (Copy makes this simple).
- The inner `item_type_owned` formatter usage (line 191) → `toggle_kind` (uses Display).
- The error-arm `Self::starred_revert_message(revert_id, &revert_type, current_starred)` (line 216) → `(.., toggle_kind, ..)`.

### 5. Migrate the rating handler (lines 395-484)

Same shape as handle_toggle_star — find every `info.item_type` / `item_type_owned` / `revert_type` site (lines 427, 432, 434, 436, 462, 477) and convert via the same Copy-eliding pattern.

### 6. Verify

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo +nightly fmt --all -- --check
```

`cargo build` requires Lane A to be present; if you branched from a post-Lane-A `main`, this passes. If `cargo test` fails because Lane B/C aren't merged yet, that's expected — Lane D's individual file `src/update/hotkeys/star_rating.rs` compiles in isolation against the Lane A symbol, but the wider crate won't build until B+C merge.

For local verification you can run the single-file check:

```
cargo check --bin nokkvi 2>&1 | grep -E 'error|warning' | grep 'star_rating.rs' | head
```

If your touched file is clean and the only errors are in `update/components.rs` / `update/{albums,artists,…}.rs` (Lanes B/C territory), you're good.

### 7. Commit

One commit, conventional format. Suggested message:

    refactor(hotkeys): migrate CenterItemInfo to ItemKind, drop owned-string clones

    Per `.agent/plans/itemkind-migration.md` Lane D:
    - CenterItemInfo.item_type: &'static str -> kind: ItemKind.
    - 8 literal "song"/"album"/"artist" sites in get_center_item_info
      flipped to ItemKind::*.
    - handle_toggle_star and the rating handler drop the
      `let item_type_owned = info.item_type.to_string()` and
      `let revert_type = info.item_type.to_string()` lines —
      ItemKind is Copy, so the async-closure capture trivially
      copies it without the round-trip through String.

    Net ~6 LOC reduction. The Display impl from Lane A handles every
    `info.item_type` formatter interpolation site without a textual
    change to the format string.

    Part of `.agent/plans/itemkind-migration.md` (Lane D, hotkeys).

Skip the Co-Authored-By trailer per global instructions.

### 8. Reporting

End with: commit hash, file LOC before/after, the count of `item_type` references remaining (should be 0). If `revert_type` or `item_type_owned` survives anywhere in the file post-edit, that's a bug — flag it and fix.

## What NOT to touch

- Anything outside `src/update/hotkeys/star_rating.rs`. The other hotkey files (`src/update/hotkeys/{global,navigation,mod}.rs` and the rest) don't carry `item_type`.
- `data/`, `src/views/`, `src/update/components.rs`, `src/update/{albums,artists,genres,playlists,queue,similar,songs}.rs`. Other lanes own those.
- `.agent/`.

## If blocked

- If `info.item_type` or `info.kind` is referenced elsewhere in the file beyond the lines listed (handle_toggle_star + rating handler): list the surprise sites and migrate them too.
- If the async-closure capture pattern is awkward (you find yourself reaching for `info.kind` after `info` was moved): introduce a `let kind = info.kind;` early, just like `revert_id`/`current_starred` are pulled out today.
- If clippy flags `unused_variables` on a destructured `info` field after migration: clean it up.
````

### lane-e-audit-progress

worktree: (uses main directly — single-file edit, no parallel work)
branch: docs/itemkind-audit-close
effort: max
permission-mode: bypassPermissions

````
Task: implement Lane E of the ItemKind migration plan — flip three rows in `.agent/audit-progress.md` to ✅ done with explicit commit refs from Lanes A+B+C+D.

Plan doc: /home/foogs/nokkvi/.agent/plans/itemkind-migration.md (section 9).

This lane is a single docs commit on `main` (no worktree). Run from `/home/foogs/nokkvi/`.

## What to do

### 1. Verify the four prior lanes have landed on main

- `git log --oneline main -20 | head -20` should show recent commits referencing the migration. Confirm by `rg`-ing for the phrase `ItemKind` in the commit log:
  ```
  git log --oneline main | head -20 | xargs -I {} sh -c 'git show --stat {} | head -3' | rg ItemKind
  ```
- Collect the commit refs:
  - Lane A foundation: 1 commit (`enum ItemKind` + length anchor + tests).
  - Lane B variants: 1-2 commits (Action variant types + literal callers).
  - Lane C helpers: 3 commits (exhaustive matches; task helper signatures; consumers + tests).
  - Lane D hotkeys: 1 commit (CenterItemInfo migration).

### 2. Edit `.agent/audit-progress.md`

Per plan §9. Three rows flip from open to done:

**§7 row 5** (line ~25):
- Status: `❌ open` → `✅ done`.
- Bold the rank: `5` → `**5**`.
- Bold the item title: `\`enum ItemKind\` to replace \`item_type: &str\`` → `**\`enum ItemKind\` to replace \`item_type: &str\`**`.
- Bold the status: `**✅ done**`.
- Replace the evidence column with the verbose multi-lane block from plan §9 (the commit message body without the leading "docs(audit):" line — adapt to the row's wiki-table style by keeping it on one line with `<br>` separators if the existing rows do, or paragraphing if they don't — match the format used by §7 row 6 (pending-expand) and §7 row 7 (orchestrator-split), which both have multi-paragraph evidence cells).

**§3 row 6** (line ~67):
- Status: `❓ stale path` → `✅ done`.
- Bold pattern matches other completed §3 rows (rank `**6**`, item bold, status bold).
- Evidence: `Same as §7 #5. The audit-progress §7 #4 'Migrate \`hotkeys/star_rating.rs\`' note about the stale path is unrelated — that item refers to inline-rebuild routing, not the ItemKind migration.`

**§4 row 2** (line ~80):
- Status: `❌ open` → `✅ done`.
- Bold pattern.
- Evidence: `Same as §7 #5.`

### 3. Verify the doc renders correctly

`cat .agent/audit-progress.md | head -80` and visually confirm the three flipped rows. Verify table column alignment isn't broken (markdown tables tolerate ragged cells but strict pipe-counts matter).

### 4. Commit

```
git add .agent/audit-progress.md
git commit -m "docs(audit): close §7 #5 / Drift #2 after ItemKind migration lands

Replaces the '❌ open' status on §7 row 5, §3 row 6, and §4 row 2
with explicit commit refs for the four-lane fanout from
.agent/plans/itemkind-migration.md (2026-05-09):

- Lane A (foundation): <commit-A> — enum ItemKind in
  data/src/types/item_kind.rs with api_str() / Display / ALL,
  paired const _: length anchors against BATCH_ITEM_VARIANT_COUNT.
- Lane B (action variants): <commits-B> — 4 view Action enums
  flipped from &'static str to ItemKind; 14 literal swaps.
- Lane C (helpers + consumers): <commits-C> — exhaustive matches
  on ItemKind in starred/rating revert helpers (the two
  '_ => …Song' wildcards in components.rs:790,800 are gone);
  star_item_task/set_item_rating_task signatures take ItemKind;
  7 update/*.rs consumer sites + 8 new tests.
- Lane D (hotkeys): <commit-D> — CenterItemInfo.kind: ItemKind;
  ~6 LOC reduction from dropped revert_type clones.

Bug class closed: typing 'alubm' no longer compiles. Adding a
future ItemKind variant breaks the build at every match site.
Adding a 6th BatchItem variant fails the const-anchor.

§3 row 6 ('Hotkey star/rating boilerplate') and §4 row 2
('item_type: &str carrying entity kind') flip to ✅ done in the
same commit since they reference the same migration."
```

Substitute `<commit-A>`, `<commits-B>`, `<commits-C>`, `<commit-D>` with the actual hashes.

Skip the Co-Authored-By trailer per global instructions.

### 5. Reporting

End with the commit hash and a one-line summary of the three flipped rows.

## What NOT to touch

- Any other audit row.
- Any other file.

## If blocked

- If the plan reference cell formatting in §7 row 6 or §7 row 7 differs from what you can match: copy the structure of whichever existing completed row is closest in length. Consistency with the existing table style matters more than matching the plan's literal text.
- If the merge order of Lanes B and C produced an awkward intermediate state on main (a commit that doesn't compile in isolation): note that in the evidence cell as a deliberate cross-lane interlock, but verify the tip of main compiles cleanly.
````
