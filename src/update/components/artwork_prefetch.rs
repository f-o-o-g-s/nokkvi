//! Canonical artwork-prefetch helpers shared by the per-view update handlers.
//!
//! Extracted from `components` as the one cohesive, path-reached unit in that
//! module (its other helpers are `Nokkvi` methods that resolve by type, not
//! path). These are the single authoritative implementation of slot-list
//! artwork prefetch — views call them via the `components::` re-export instead
//! of inline loops. The version-aware [`should_refetch`] gate (N17) is the
//! shared dedup core every surface routes through.
use std::collections::{HashMap, HashSet};

use iced::Task;
use nokkvi_data::{
    backend::albums::{AlbumUIViewData, AlbumsService},
    utils::artwork_url::THUMBNAIL_SIZE,
};

use crate::{
    app_message::{ArtworkMessage, Message, MiniArt},
    widgets::SlotListView,
};

/// Version-aware prefetch dedup gate (N17) with a negative-cache short-circuit.
///
/// Returns `true` when an album's 80px thumbnail should be (re-)fetched:
/// - the slot is warmed but the recorded `updated_at` differs from the one the
///   current URL carries — i.e. the server-side cover changed; OR
/// - the slot was never warmed (`id` absent from `album_art`) AND `id` is not in
///   the negative cache at the current version.
///
/// `cached_ids` are the live `album_art` keys; `versions` is the sibling
/// `album_art_versions` map; `failed` is the `failed_art` negative cache
/// (`id -> the updated_at that returned no image`). Checking `album_art`
/// membership (not just the version map) guards the documented eviction skew:
/// `album_art` evicts silently at capacity, so a stale version entry whose
/// handle is gone must still count as a miss. The negative-cache check is
/// version-aware too: a changed `updated_at` re-attempts a previously-dead id.
pub(crate) fn should_refetch(
    cached_ids: &HashSet<&String>,
    versions: &HashMap<String, Option<String>>,
    failed: &HashMap<String, Option<String>>,
    id: &String,
    updated_at: &Option<String>,
) -> bool {
    if cached_ids.contains(id) {
        // Warmed: refetch only when the recorded version no longer matches.
        return versions.get(id) != Some(updated_at);
    }
    // Handle absent (never warmed, or evicted). Re-fetch UNLESS this id already
    // failed at THIS exact version — a known-dead cover must not be re-queued on
    // every scroll/resize. A changed updated_at (server cover added) bypasses the
    // negative entry, mirroring the version branch; logout and a user "Refresh
    // Artwork" also drop the entry.
    failed.get(id) != Some(updated_at)
}

/// The album-coherent version that the PASSIVE 80px-thumbnail surfaces (queue,
/// song-mini, similar, playlist editor) feed into the album_id-keyed
/// [`should_refetch`] gate.
///
/// Those rows only carry a PER-SONG `updated_at` (`Song.updated_at`, a
/// per-mediafile timestamp) — not an album cover version. Feeding that per-song
/// value into the album_id-keyed `album_art_versions` map makes the recorded
/// version oscillate across a single album's tracks, so the gate keeps missing
/// and re-puts identical bytes (a fresh `Handle::from_bytes` texture → flicker).
///
/// Returning a constant `None` makes the recorded version album-coherent: every
/// track of one album maps to the same value, so once an album slot is warm the
/// gate stays warm (until `album_art` evicts, where the membership branch still
/// forces a correct re-warm). The argument is taken (and ignored) so the call
/// sites read as "we deliberately drop the per-song timestamp here".
///
/// N17 (server cover-change invalidation) is retained on the Albums view and
/// the Artists/Genres expansion paths, which pass the album-coherent
/// `album.updated_at` directly and do not route through this helper.
pub(crate) fn passive_artwork_version(_per_song_updated_at: &Option<String>) -> Option<String> {
    None
}

/// Generate artwork prefetch tasks for a slot list viewport.
///
/// This is the single authoritative implementation of artwork prefetching.
/// All slot-list-based views should use this instead of inline loops.
///
/// The `extract_id_url` closure returns `(album_id, version, url)`: the
/// `version` feeds the version-aware [`should_refetch`] gate so a changed
/// server cover re-fetches even when the bare id is already cached (N17).
///
/// The album-coherent surfaces (Albums view, Artists/Genres expansion) pass the
/// album's `updated_at` here, keeping live cover invalidation. The PASSIVE
/// surfaces (queue, playlist editor) only carry a per-song `updated_at`, which
/// would oscillate this album_id-keyed gate; they pass
/// [`passive_artwork_version`] (a constant `None`) instead, so they use id-only
/// dedup and re-warm the current cover on the next cold load.
pub(crate) fn prefetch_album_artwork_tasks<F, T>(
    slot_list: &SlotListView,
    items: &[T],
    cached_ids: &HashSet<&String>,
    versions: &HashMap<String, Option<String>>,
    failed: &HashMap<String, Option<String>>,
    albums_vm: AlbumsService,
    extract_id_url: F,
) -> Vec<Task<Message>>
where
    F: Fn(&T) -> (String, Option<String>, String),
{
    let total = items.len();
    if total == 0 {
        return Vec::new();
    }

    let mut already_queued = HashSet::new();

    slot_list
        .prefetch_indices(total)
        .filter_map(|idx| items.get(idx))
        .filter_map(|item| {
            let (id, updated_at, url) = extract_id_url(item);
            // Nothing to fetch for an empty id/URL (e.g. a queue song with no
            // album_id). Skip the no-op fetch — matches the quad path's
            // id.is_empty() guard and the song variant's None-skip. Otherwise
            // fetch_artwork_by_url returns a deterministic "empty url" error that
            // classifies as Transient (not a NonImageResponse), so it is never
            // negatively cached and re-queues on every viewport pass.
            if id.is_empty() || url.is_empty() {
                return None;
            }
            // Skip if version-warm (id cached AND recorded version matches) or
            // already queued in this batch; a changed updated_at is a miss.
            if !should_refetch(cached_ids, versions, failed, &id, &updated_at)
                || already_queued.contains(&id)
            {
                None
            } else {
                already_queued.insert(id.clone());
                Some((id, updated_at, url))
            }
        })
        .map(|(id, updated_at, url)| {
            let vm = albums_vm.clone();
            Task::perform(
                async move {
                    let art = MiniArt::from_fetch(vm.fetch_artwork_by_url(&url).await);
                    (id, updated_at, art)
                },
                |(id, updated_at, art)| {
                    Message::Artwork(ArtworkMessage::Loaded(id, updated_at, art))
                },
            )
        })
        .collect()
}

/// Generate song artwork prefetch tasks for a slot list viewport.
///
/// Variant of `prefetch_album_artwork_tasks` for songs that have
/// `Option<album_id>`. Generic over the slice element type — Songs page
/// passes `SongUIViewData`, Similar page passes raw `Song`. The
/// `extract_album_id` closure returns `(album_id, version)`, where `version`
/// feeds both the dedup gate and the fetch URL's `_u=` cache-buster.
///
/// These are PASSIVE song-mini surfaces (Songs page, Similar page): their rows
/// carry only a per-song `updated_at`, which would oscillate the album_id-keyed
/// gate, so they pass [`passive_artwork_version`] (a constant `None`) for the
/// version. Id-only dedup is used here; the current cover is re-warmed on the
/// next cold load. A `None` version also yields the documented empty-`_u=` URL
/// shape, which still returns the current cover (there is no client-side
/// artwork response cache). Dispatches
/// `Message::Artwork(ArtworkMessage::SongMiniLoaded)`.
pub(crate) fn prefetch_song_artwork_tasks<T, F>(
    slot_list: &SlotListView,
    songs: &[T],
    cached_ids: &HashSet<&String>,
    versions: &HashMap<String, Option<String>>,
    failed: &HashMap<String, Option<String>>,
    albums_vm: AlbumsService,
    extract_album_id: F,
) -> Vec<Task<Message>>
where
    F: Fn(&T) -> Option<(&String, Option<String>)>,
{
    let total = songs.len();
    if total == 0 {
        return Vec::new();
    }

    let mut already_queued = HashSet::new();

    slot_list
        .prefetch_indices(total)
        .filter_map(|idx| songs.get(idx))
        .filter_map(|song| {
            extract_album_id(song).and_then(|(id, updated_at)| {
                if !should_refetch(cached_ids, versions, failed, id, &updated_at)
                    || already_queued.contains(id)
                {
                    None
                } else {
                    already_queued.insert(id.clone());
                    Some((id.clone(), updated_at))
                }
            })
        })
        .map(|(album_id, updated_at)| {
            let vm = albums_vm.clone();
            let id = album_id;
            Task::perform(
                async move {
                    let art = MiniArt::from_fetch(
                        vm.fetch_album_artwork(&id, Some(THUMBNAIL_SIZE), updated_at.as_deref())
                            .await,
                    );
                    (id, updated_at, art)
                },
                |(id, updated_at, art)| {
                    Message::Artwork(crate::app_message::ArtworkMessage::SongMiniLoaded(
                        id, updated_at, art,
                    ))
                },
            )
        })
        .collect()
}

/// Generate 80px quad-tile prefetch tasks for a slot list viewport.
///
/// Variant of [`prefetch_album_artwork_tasks`] for rows that render a 2×2
/// quad of album covers (playlists, genres): each viewport item contributes
/// its first `QUAD_TILE_COUNT` distinct `artwork_album_ids` instead of a
/// single id.
/// Tiles are fetched by bare album id at `THUMBNAIL_SIZE` — the same
/// album-id-keyed URL shape the queue's song minis use — and land via
/// `ArtworkMessage::Loaded` in the shared `album_art` LRU, so tiles warmed by
/// any other surface are reused for free.
///
/// Dedup is strictly membership-based, on purpose:
/// - `cached_ids` membership (never the version map — a quad re-dispatch must
///   not treat an album-coherent `Some(updated_at)` entry as a miss and stomp
///   it back to `None`, which would ping-pong fetches against the Albums
///   view's version-aware gate);
/// - `pending_ids` in-flight gate (this prefetch re-fires on every scroll
///   step and collage event, so without it a cold viewport would duplicate
///   every still-loading request per step). Returned queued ids must be
///   inserted into that set by the caller; `handle_artwork_loaded` releases
///   them.
///
/// Fetches go through the retry wrapper: quad tiles have no scroll-refetch
/// guarantee on a stationary viewport, so a single throttled request must
/// not pin a row to its single-mini fallback until the next dispatch.
///
/// Items whose `artwork_album_ids` are still unresolved contribute nothing;
/// the `CollageAlbumIdsLoaded` / `CollageLoaded` handlers re-dispatch this
/// prefetch once ids land.
///
/// Returns `(queued_ids, tasks)` — the collage-loader convention.
pub(crate) fn prefetch_quad_album_artwork_tasks<T, F>(
    slot_list: &SlotListView,
    items: &[T],
    cached_ids: &HashSet<&String>,
    failed: &HashMap<String, Option<String>>,
    pending_ids: &HashSet<String>,
    albums_vm: AlbumsService,
    extract_album_ids: F,
) -> (Vec<String>, Vec<Task<Message>>)
where
    F: Fn(&T) -> &[String],
{
    use crate::services::collage_artwork::QUAD_TILE_COUNT;

    let total = items.len();
    if total == 0 {
        return (Vec::new(), Vec::new());
    }

    let mut queued_ids: Vec<String> = Vec::new();
    let mut already_queued: HashSet<String> = HashSet::new();
    let mut tasks: Vec<Task<Message>> = Vec::new();

    for idx in slot_list.prefetch_indices(total) {
        let Some(item) = items.get(idx) else { continue };
        for id in extract_album_ids(item).iter().take(QUAD_TILE_COUNT) {
            if id.is_empty()
                || already_queued.contains(id)
                || cached_ids.contains(id)
                || pending_ids.contains(id)
                || failed.contains_key(id)
            {
                continue;
            }
            already_queued.insert(id.clone());
            queued_ids.push(id.clone());
            let vm = albums_vm.clone();
            let id = id.clone();
            tasks.push(Task::perform(
                async move {
                    let art = MiniArt::from_fetch(
                        vm.fetch_album_artwork_with_retry(&id, Some(THUMBNAIL_SIZE), None)
                            .await,
                    );
                    (id, art)
                },
                |(id, art)| Message::Artwork(ArtworkMessage::Loaded(id, None, art)),
            ));
        }
    }

    (queued_ids, tasks)
}

/// Non-slot-list sibling of [`prefetch_quad_album_artwork_tasks`]: fetch quad
/// tiles for a flat set of album ids that is NOT viewport-driven (the Harbour
/// playlist shelf has no `SlotListView`). Same membership-based dedup
/// (`cached_ids` / `pending_ids` / `failed`), same retry-wrapped
/// `fetch_album_artwork_with_retry` at `THUMBNAIL_SIZE`, same `(queued_ids,
/// tasks)` return convention — the caller inserts `queued_ids` into
/// `album_art_pending` and `handle_artwork_loaded` releases them.
///
/// `album_ids_per_item` yields each item's ordered album ids; only the first
/// `QUAD_TILE_COUNT` of each are taken (a quad needs at most four).
pub(crate) fn quad_album_artwork_tasks_for_ids<'a, I>(
    cached_ids: &HashSet<&String>,
    failed: &HashMap<String, Option<String>>,
    pending_ids: &HashSet<String>,
    albums_vm: AlbumsService,
    album_ids_per_item: I,
) -> (Vec<String>, Vec<Task<Message>>)
where
    I: IntoIterator<Item = &'a [String]>,
{
    use crate::services::collage_artwork::QUAD_TILE_COUNT;

    let mut queued_ids: Vec<String> = Vec::new();
    let mut already_queued: HashSet<String> = HashSet::new();
    let mut tasks: Vec<Task<Message>> = Vec::new();

    for ids in album_ids_per_item {
        for id in ids.iter().take(QUAD_TILE_COUNT) {
            if id.is_empty()
                || already_queued.contains(id)
                || cached_ids.contains(id)
                || pending_ids.contains(id)
                || failed.contains_key(id)
            {
                continue;
            }
            already_queued.insert(id.clone());
            queued_ids.push(id.clone());
            let vm = albums_vm.clone();
            let id = id.clone();
            tasks.push(Task::perform(
                async move {
                    let art = MiniArt::from_fetch(
                        vm.fetch_album_artwork_with_retry(&id, Some(THUMBNAIL_SIZE), None)
                            .await,
                    );
                    (id, art)
                },
                |(id, art)| Message::Artwork(ArtworkMessage::Loaded(id, None, art)),
            ));
        }
    }

    (queued_ids, tasks)
}

/// Fan-out 80px album artwork fetches for albums newly delivered into a
/// view's expansion children (Artists→Album, Genres→Album). Skips ids
/// already in the cache; each surviving id dispatches an
/// `ArtworkMessage::Loaded` so the centralized `handle_artwork_loaded`
/// arm puts the handle into `album_art` exactly the way Albums view does.
///
/// Callers pass `(album.id, album.updated_at, album.artwork_url)` triples —
/// the URL is pre-built by `AlbumUIViewData::from_album` from `album.cover_art`
/// (with `album.id` as fallback). For albums whose artwork lives on a
/// media file (`cover_art = "mf-…"`) this matters — passing only the
/// album id would build the wrong URL and the fetch would return empty. The
/// `updated_at` is forwarded into the `Loaded` message so the recorded
/// `album_art_versions` entry matches the URL's cache-buster (N17).
///
/// Each fetch goes through `fetch_artwork_by_url_with_retry` (3 attempts,
/// 100 ms / 200 ms backoff). Without retries, large expansions (e.g. a
/// genre with 150+ albums) reliably drop 1–2 thumbnails because
/// Navidrome's `getCoverArt` throttle middleware rejects requests that
/// exceed its in-flight backlog cap. Genre/artist expansions have no
/// scroll-triggered re-fetch path, so a single dropped fetch leaves a
/// permanently-blank slot until the next expansion.
///
/// `pending_ids` is the `album_art_pending` in-flight set: a genre's
/// expansion children lead with the SAME albums its row quad is warming
/// (both derive from the name-ASC `/api/album` listing), and FocusAndExpand
/// fires the quad dispatch and the expansion fan-out in the same event
/// cluster — without this filter the fan-out duplicates every quad fetch
/// still in flight.
pub(crate) fn expansion_album_artwork_tasks(
    cached_ids: &HashSet<&String>,
    versions: &HashMap<String, Option<String>>,
    failed: &HashMap<String, Option<String>>,
    pending_ids: &HashSet<String>,
    albums_vm: AlbumsService,
    album_ids_urls: Vec<(String, Option<String>, String)>,
) -> Vec<Task<Message>> {
    album_ids_urls
        .into_iter()
        .filter(|(id, updated_at, _)| {
            should_refetch(cached_ids, versions, failed, id, updated_at)
                && !pending_ids.contains(id)
        })
        .map(|(id, updated_at, url)| {
            let vm = albums_vm.clone();
            Task::perform(
                async move {
                    let art = MiniArt::from_fetch(vm.fetch_artwork_by_url_with_retry(&url).await);
                    (id, updated_at, art)
                },
                |(id, updated_at, art)| {
                    Message::Artwork(ArtworkMessage::Loaded(id, updated_at, art))
                },
            )
        })
        .collect()
}

/// Project newly-loaded expansion children into the
/// `(id, updated_at, artwork_url)` triples [`expansion_album_artwork_tasks`]
/// consumes (the doc comment above documents the triple contract). Shared by
/// the Artists and Genres handler prologues, which capture the triples from
/// the `AlbumsLoaded` message before the page update consumes it.
pub(crate) fn expansion_child_album_ids(
    albums: &[AlbumUIViewData],
) -> Vec<(String, Option<String>, String)> {
    albums
        .iter()
        .map(|a| (a.id.clone(), a.updated_at.clone(), a.artwork_url.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::expansion_child_album_ids;
    use crate::test_helpers::make_album;

    #[test]
    fn expansion_child_album_ids_maps_id_version_url_triples() {
        let mut a1 = make_album("a1", "Alpha", "Artist");
        a1.updated_at = Some("2026-01-01".to_string());
        a1.artwork_url = "http://server/art/a1".to_string();
        let mut a2 = make_album("a2", "Beta", "Artist");
        a2.artwork_url = "http://server/art/a2".to_string();

        let triples = expansion_child_album_ids(&[a1, a2]);

        assert_eq!(
            triples,
            vec![
                (
                    "a1".to_string(),
                    Some("2026-01-01".to_string()),
                    "http://server/art/a1".to_string()
                ),
                ("a2".to_string(), None, "http://server/art/a2".to_string()),
            ]
        );
    }
}
