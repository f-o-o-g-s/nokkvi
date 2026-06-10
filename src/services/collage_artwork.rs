//! Collage artwork loading service for genres and playlists
//!
//! Provides a unified implementation for loading multi-album collage artwork.
//!
//! ## Design
//!
//! Both genres and playlists display collage artwork composed of up to 9 album covers.
//! The loading logic follows the same pattern:
//! 1. Check if already pending (skip)
//! 2. Check if fully cached in memory (skip)
//! 3. Otherwise, dispatch a network fetch
//!
//! This module extracts that shared pattern into reusable functions.

use std::collections::{HashMap, HashSet};

use iced::{Task, widget::image};
use nokkvi_data::backend::auth::AuthGateway;
use tracing::trace;

use crate::{app_message::Message, widgets::SlotListView};

/// Result tuple from `load_visible_artwork`:
/// - `pending_inserts`: Item IDs to mark as pending
/// - `tasks`: Network fetch tasks to execute
pub(crate) type LoadArtworkResult = (Vec<String>, Vec<Task<Message>>);

/// Number of tiles in the small 2×2 quad thumbnail (playlist slot rows and
/// the queue's "Playing From" strip cover).
pub(crate) const QUAD_TILE_COUNT: usize = 4;

/// Resolve the 80px tile handles for a 2×2 quad thumbnail.
///
/// `distinct_album_ids` must already be deduplicated (playlist
/// `artwork_album_ids` are unique by construction; queue-derived ids go
/// through [`first_distinct_album_ids`]). The first `QUAD_TILE_COUNT` ids are
/// looked up in the album-id-keyed 80px `album_art` snapshot.
///
/// Returns `None` when fewer than 2 distinct ids exist (single-album surfaces
/// keep their current single-cover look, mirroring the large panel's
/// ≤1-album gating) or when any tile handle is still missing (callers fall
/// back to the single mini until the prefetch lands — the same atomic
/// mini→collage upgrade the large panel does, avoiding half-filled grids).
pub(crate) fn resolve_quad_handles<'a, S: AsRef<str>>(
    distinct_album_ids: &[S],
    album_art: &'a HashMap<String, image::Handle>,
) -> Option<Vec<&'a image::Handle>> {
    if distinct_album_ids.len() < 2 {
        return None;
    }
    distinct_album_ids
        .iter()
        .take(QUAD_TILE_COUNT)
        .map(|id| album_art.get(id.as_ref()))
        .collect()
}

/// First `QUAD_TILE_COUNT` distinct, non-empty album ids in iteration order.
///
/// Used to derive the queue banner's quad tiles from the unfiltered queue
/// songs (queue order == playlist track order at enqueue time, matching the
/// "first albums of the playlist" the slot rows show).
pub(crate) fn first_distinct_album_ids<'a>(ids: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut out: Vec<&'a str> = Vec::new();
    for id in ids {
        if id.is_empty() || out.contains(&id) {
            continue;
        }
        out.push(id);
        if out.len() == QUAD_TILE_COUNT {
            break;
        }
    }
    out
}

/// Trait for items that display collage artwork (genres, playlists)
/// Re-exported from data crate for use in GUI service code
pub(crate) use nokkvi_data::types::collage_artwork::CollageArtworkItem;

/// Context for loading collage artwork - bundles all the state references needed
pub(crate) struct CollageArtworkContext<'a> {
    /// Slot list view for determining visible slots
    pub slot_list: &'a SlotListView,
    /// IDs of items currently being loaded (prevents duplicate requests)
    pub pending_ids: &'a HashSet<String>,
    /// In-memory mini artwork cache
    pub memory_artwork: &'a HashMap<String, image::Handle>,
    /// In-memory collage artwork cache
    pub memory_collage: &'a HashMap<String, Vec<image::Handle>>,
}

/// Result of checking in-memory state for a collage item.
///
/// The dedicated genre/playlist disk caches were retired with the HTTP-cache
/// migration; sync disk hits are no longer possible (the cached client serves
/// async). All cache misses become `NeedNetwork`, which the caller then routes
/// through `AlbumsService::fetch_album_artwork`.
#[derive(Debug)]
pub(crate) enum CacheCheckResult {
    /// Both mini and collage already in memory - skip
    FullyCached,
    /// Need network load
    NeedNetwork,
    /// Already pending - skip
    AlreadyPending,
}

pub(crate) fn check_cache<T: CollageArtworkItem>(
    item: &T,
    ctx: &CollageArtworkContext,
) -> CacheCheckResult {
    let id = item.id();

    if ctx.pending_ids.contains(id) {
        return CacheCheckResult::AlreadyPending;
    }
    if ctx.memory_artwork.contains_key(id) && ctx.memory_collage.contains_key(id) {
        return CacheCheckResult::FullyCached;
    }
    CacheCheckResult::NeedNetwork
}

/// Generate tasks to load visible collage artwork for the slot list viewport.
///
/// Splits the fan-out so only the centered item fetches the full 3×3 collage
/// (which is the only slot that actually renders the right-side panel);
/// every other visible item fetches its mini only. With a worst-case 25-slot
/// viewport this drops total request volume from ~250 to ~25 and keeps the
/// burst well under Navidrome's `getCoverArt` throttle backlog.
///
/// # Returns
/// A tuple of:
/// * `pending_inserts` - Item IDs to mark as pending
/// * `tasks` - Network fetch tasks to execute
///
/// # Arguments
/// * `items` - Full list of items
/// * `ctx` - Context with cache/state references
/// * `auth_vm` - Auth view model for fetching credentials
/// * `center_id` - ID of the centered slot (gets the full collage fetch);
///   every other prefetch index gets a mini-only fetch
/// * `create_full_message` - Closure for the centered item (`LoadCollage`)
/// * `create_mini_message` - Closure for non-centered items (`LoadCollageMini`)
pub(crate) fn load_visible_artwork<T, FFull, FMini>(
    items: &[T],
    ctx: &CollageArtworkContext,
    auth_vm: AuthGateway,
    center_id: Option<&str>,
    create_full_message: FFull,
    create_mini_message: FMini,
) -> LoadArtworkResult
where
    T: CollageArtworkItem,
    FFull: Fn(String, String, String, Vec<String>) -> Message + Clone + Send + 'static,
    FMini: Fn(String, String, String, Vec<String>) -> Message + Clone + Send + 'static,
{
    let total = items.len();
    if total == 0 {
        return (Vec::new(), Vec::new());
    }

    let mut pending_inserts: Vec<String> = Vec::new();
    let mut tasks: Vec<Task<Message>> = Vec::new();

    let indices_to_load: Vec<usize> = ctx.slot_list.prefetch_indices(total).collect();

    for idx in indices_to_load {
        if let Some(item) = items.get(idx) {
            match check_cache(item, ctx) {
                CacheCheckResult::FullyCached | CacheCheckResult::AlreadyPending => continue,
                CacheCheckResult::NeedNetwork => {
                    let id = item.id().to_string();
                    pending_inserts.push(id.clone());

                    let auth_vm_clone = auth_vm.clone();
                    let album_ids = item.artwork_album_ids().to_vec();
                    let is_center = center_id.is_some_and(|cid| cid == id);
                    let create_full = create_full_message.clone();
                    let create_mini = create_mini_message.clone();
                    tasks.push(Task::perform(
                        async move {
                            let (server_url, subsonic_credential) =
                                auth_vm_clone.server_config().await;
                            (id, server_url, subsonic_credential, album_ids, is_center)
                        },
                        move |(id, url, cred, album_ids, is_center)| {
                            if is_center {
                                create_full(id, url, cred, album_ids)
                            } else {
                                create_mini(id, url, cred, album_ids)
                            }
                        },
                    ));
                }
            }
        }
    }

    trace!(
        "Collage artwork: {} tasks, {} pending",
        tasks.len(),
        pending_inserts.len()
    );

    (pending_inserts, tasks)
}

/// Generate tasks to preload collage artwork for the slot list viewport
///
/// This handles the PreloadArtwork action pattern - fetches credentials once
/// then emits a batch-ready message with all IDs that need loading.
///
/// # Arguments
/// * `items` - Full list of items
/// * `ctx` - Context with cache/state references  
/// * `auth_vm` - Auth view model for fetching credentials
/// * `create_batch_message` - Closure to create the batch-ready Message variant
///
/// # Returns
/// * `pending_inserts` - Item IDs to mark as pending immediately
/// * `task` - Optional task to fetch credentials and emit batch message
pub(crate) fn preload_artwork<T, F>(
    items: &[T],
    ctx: &CollageArtworkContext,
    auth_vm: AuthGateway,
    create_batch_message: F,
) -> (Vec<String>, Option<Task<Message>>)
where
    T: CollageArtworkItem,
    F: Fn(Vec<String>, String, String) -> Message + Send + 'static,
{
    let total = items.len();
    if total == 0 {
        return (Vec::new(), None);
    }

    let mut ids_to_load: Vec<String> = Vec::new();

    for idx in ctx.slot_list.prefetch_indices(total) {
        if let Some(item) = items.get(idx) {
            let id = item.id();
            if !ctx.memory_artwork.contains_key(id) && !ctx.pending_ids.contains(id) {
                ids_to_load.push(id.to_string());
            }
        }
    }

    if ids_to_load.is_empty() {
        return (Vec::new(), None);
    }

    let pending_inserts = ids_to_load.clone();

    let task = Task::perform(
        async move {
            let (server_url, subsonic_credential) = auth_vm.server_config().await;
            (ids_to_load, server_url, subsonic_credential)
        },
        move |(ids, url, cred)| create_batch_message(ids, url, cred),
    );

    (pending_inserts, Some(task))
}

#[cfg(test)]
mod tests {
    use nokkvi_data::backend::auth::AuthGateway;

    use super::*;
    use crate::widgets::SlotListView;

    struct FakeItem {
        id: String,
        album_ids: Vec<String>,
    }

    impl CollageArtworkItem for FakeItem {
        fn id(&self) -> &str {
            &self.id
        }
        fn artwork_album_ids(&self) -> &[String] {
            &self.album_ids
        }
    }

    fn fake(id: &str) -> FakeItem {
        FakeItem {
            id: id.to_string(),
            album_ids: vec![format!("{id}-album-0")],
        }
    }

    /// Empty items list short-circuits to two empty vecs. Pins the 2-tuple
    /// shape that replaced the legacy 3-tuple (the middle `cache_inserts`
    /// slot was always empty after the disk cache was retired).
    #[test]
    fn load_visible_artwork_empty_items_returns_empty_pair() {
        let slot_list = SlotListView::new();
        let memory_artwork = HashMap::new();
        let memory_collage = HashMap::new();
        let pending_ids = HashSet::new();
        let ctx = CollageArtworkContext {
            slot_list: &slot_list,
            pending_ids: &pending_ids,
            memory_artwork: &memory_artwork,
            memory_collage: &memory_collage,
        };
        let auth_vm = AuthGateway::new().expect("auth gateway");

        let (pending_inserts, tasks) = load_visible_artwork::<FakeItem, _, _>(
            &[],
            &ctx,
            auth_vm,
            None,
            |_, _, _, _| panic!("full closure should not run for empty input"),
            |_, _, _, _| panic!("mini closure should not run for empty input"),
        );

        assert!(pending_inserts.is_empty());
        assert!(tasks.is_empty());
    }

    fn handle() -> image::Handle {
        image::Handle::from_bytes(Vec::<u8>::new())
    }

    fn art_map(ids: &[&str]) -> HashMap<String, image::Handle> {
        ids.iter().map(|id| (id.to_string(), handle())).collect()
    }

    /// Fewer than 2 distinct album ids → no quad; the surface keeps its
    /// single-cover look (mirrors the large panel's ≤1-album gating).
    #[test]
    fn resolve_quad_handles_requires_two_distinct_ids() {
        let art = art_map(&["a1"]);
        assert!(resolve_quad_handles::<String>(&[], &art).is_none());
        assert!(resolve_quad_handles(&["a1".to_string()], &art).is_none());
    }

    /// 2–4 distinct cached ids resolve to that many tiles, in id order; the
    /// renderer's modulo wrap fills the remaining cells.
    #[test]
    fn resolve_quad_handles_returns_all_cached_tiles_in_order() {
        let art = art_map(&["a1", "a2", "a3"]);
        let ids = ["a1".to_string(), "a2".to_string()];
        let tiles = resolve_quad_handles(&ids, &art).expect("2 cached ids resolve");
        assert_eq!(tiles.len(), 2);

        let ids3 = ["a1".to_string(), "a2".to_string(), "a3".to_string()];
        let tiles3 = resolve_quad_handles(&ids3, &art).expect("3 cached ids resolve");
        assert_eq!(tiles3.len(), 3);
    }

    /// More than 4 ids: only the first QUAD_TILE_COUNT are used, so a missing
    /// handle beyond the fourth id must not block the quad.
    #[test]
    fn resolve_quad_handles_uses_first_four_ids_only() {
        let art = art_map(&["a1", "a2", "a3", "a4"]);
        let ids: Vec<String> = ["a1", "a2", "a3", "a4", "a5-uncached"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let tiles = resolve_quad_handles(&ids, &art).expect("first 4 cached");
        assert_eq!(tiles.len(), QUAD_TILE_COUNT);
    }

    /// Any missing tile among the first ≤4 → None (atomic upgrade: callers
    /// fall back to the single mini rather than render a half-filled grid).
    #[test]
    fn resolve_quad_handles_none_when_any_tile_uncached() {
        let art = art_map(&["a1", "a3"]);
        let ids = ["a1".to_string(), "a2".to_string(), "a3".to_string()];
        assert!(resolve_quad_handles(&ids, &art).is_none());
    }

    /// Dedup preserves first-seen order, skips empty ids, caps at
    /// QUAD_TILE_COUNT.
    #[test]
    fn first_distinct_album_ids_dedups_in_order_and_caps() {
        let ids = ["a1", "a2", "a1", "", "a3", "a2", "a4", "a5"];
        assert_eq!(
            first_distinct_album_ids(ids.into_iter()),
            vec!["a1", "a2", "a3", "a4"]
        );
    }

    /// Items already pending or fully cached skip the network — no pending
    /// markers added, no tasks dispatched.
    #[test]
    fn load_visible_artwork_skips_cached_and_pending_items() {
        let items = vec![fake("g-pending"), fake("g-cached")];
        let slot_list = SlotListView::new();
        let mut memory_artwork: HashMap<String, image::Handle> = HashMap::new();
        let mut memory_collage: HashMap<String, Vec<image::Handle>> = HashMap::new();
        memory_artwork.insert(
            "g-cached".to_string(),
            image::Handle::from_bytes(Vec::<u8>::new()),
        );
        memory_collage.insert("g-cached".to_string(), Vec::new());

        let mut pending_ids = HashSet::new();
        pending_ids.insert("g-pending".to_string());

        let ctx = CollageArtworkContext {
            slot_list: &slot_list,
            pending_ids: &pending_ids,
            memory_artwork: &memory_artwork,
            memory_collage: &memory_collage,
        };
        let auth_vm = AuthGateway::new().expect("auth gateway");

        let (pending_inserts, tasks) = load_visible_artwork(
            &items,
            &ctx,
            auth_vm,
            Some("g-pending"),
            |_, _, _, _| panic!("full closure should not run for cached/pending input"),
            |_, _, _, _| panic!("mini closure should not run for cached/pending input"),
        );

        assert!(pending_inserts.is_empty(), "no new pending markers");
        assert!(tasks.is_empty(), "no tasks dispatched");
    }
}
