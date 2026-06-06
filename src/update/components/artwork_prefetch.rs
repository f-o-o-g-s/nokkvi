//! Canonical artwork-prefetch helpers shared by the per-view update handlers.
//!
//! Extracted from `components` as the one cohesive, path-reached unit in that
//! module (its other helpers are `Nokkvi` methods that resolve by type, not
//! path). These are the single authoritative implementation of slot-list
//! artwork prefetch — views call them via the `components::` re-export instead
//! of inline loops. The version-aware [`should_refetch`] gate (N17) is the
//! shared dedup core every surface routes through.
use std::collections::{HashMap, HashSet};

use iced::{Task, widget::image};
use nokkvi_data::{backend::albums::AlbumsService, utils::artwork_url::THUMBNAIL_SIZE};

use crate::{
    app_message::{ArtworkMessage, Message},
    widgets::SlotListView,
};

/// Version-aware prefetch dedup gate (N17).
///
/// Returns `true` when an album's 80px thumbnail should be (re-)fetched:
/// - the slot was never warmed (`id` absent from `album_art`), OR
/// - the slot is warmed but the recorded `updated_at` differs from the one the
///   current URL carries — i.e. the server-side cover changed.
///
/// `cached_ids` are the live `album_art` keys; `versions` is the sibling
/// `album_art_versions` map. Checking `album_art` membership (not just the
/// version map) guards the documented eviction skew: `album_art` evicts
/// silently at capacity, so a stale version entry whose handle is gone must
/// still count as a miss.
pub(crate) fn should_refetch(
    cached_ids: &HashSet<&String>,
    versions: &HashMap<String, Option<String>>,
    id: &String,
    updated_at: &Option<String>,
) -> bool {
    if !cached_ids.contains(id) {
        // Handle absent (never warmed, or evicted) — always a miss.
        return true;
    }
    // Warmed: refetch only when the recorded version no longer matches.
    versions.get(id) != Some(updated_at)
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
            // Skip if version-warm (id cached AND recorded version matches) or
            // already queued in this batch; a changed updated_at is a miss.
            if !should_refetch(cached_ids, versions, &id, &updated_at)
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
                    let bytes = vm.fetch_artwork_by_url(&url).await.ok();
                    (id, updated_at, bytes.map(image::Handle::from_bytes))
                },
                |(id, updated_at, handle)| {
                    Message::Artwork(ArtworkMessage::Loaded(id, updated_at, handle))
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
                if !should_refetch(cached_ids, versions, id, &updated_at)
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
                    let bytes = vm
                        .fetch_album_artwork(&id, Some(THUMBNAIL_SIZE), updated_at.as_deref())
                        .await
                        .ok();
                    (id, updated_at, bytes.map(image::Handle::from_bytes))
                },
                |(id, updated_at, handle)| {
                    Message::Artwork(crate::app_message::ArtworkMessage::SongMiniLoaded(
                        id, updated_at, handle,
                    ))
                },
            )
        })
        .collect()
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
pub(crate) fn expansion_album_artwork_tasks(
    cached_ids: &HashSet<&String>,
    versions: &HashMap<String, Option<String>>,
    albums_vm: AlbumsService,
    album_ids_urls: Vec<(String, Option<String>, String)>,
) -> Vec<Task<Message>> {
    album_ids_urls
        .into_iter()
        .filter(|(id, updated_at, _)| should_refetch(cached_ids, versions, id, updated_at))
        .map(|(id, updated_at, url)| {
            let vm = albums_vm.clone();
            Task::perform(
                async move {
                    let handle = vm
                        .fetch_artwork_by_url_with_retry(&url)
                        .await
                        .ok()
                        .map(image::Handle::from_bytes);
                    (id, updated_at, handle)
                },
                |(id, updated_at, handle)| {
                    Message::Artwork(ArtworkMessage::Loaded(id, updated_at, handle))
                },
            )
        })
        .collect()
}
