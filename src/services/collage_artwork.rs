//! Collage artwork loading service for genres and playlists
//!
//! Provides a unified implementation for loading multi-album collage artwork
//! with disk cache + network fallback pattern.
//!
//! ## Design
//!
//! Both genres and playlists display collage artwork composed of up to 9 album covers.
//! The loading logic follows the same pattern:
//! 1. Check if already pending (skip)
//! 2. Check if fully cached in memory (skip)
//! 3. Check disk cache for mini artwork
//! 4. If disk cache hit but no collage, still need network for collage handles
//! 5. If disk cache miss, need full network load
//!
//! This module extracts that shared pattern into reusable functions.

use std::collections::{HashMap, HashSet};

use iced::{Task, widget::image};
use nokkvi_data::{backend::auth::AuthGateway, utils::cache::DiskCache};
use tracing::trace;

use crate::{app_message::Message, widgets::SlotListView};

/// Result tuple from `load_visible_artwork`:
/// - `pending_inserts`: Item IDs to mark as pending
/// - `cache_inserts`: (id, handle) pairs loaded from disk cache
/// - `tasks`: Network fetch tasks to execute
pub(crate) type LoadArtworkResult = (
    Vec<String>,
    Vec<(String, image::Handle)>,
    Vec<Task<Message>>,
);

/// Trait for items that display collage artwork (genres, playlists)
/// Re-exported from data crate for use in GUI service code
pub(crate) use nokkvi_data::types::collage_artwork::CollageArtworkItem;

/// Context for loading collage artwork - bundles all the state references needed
pub(crate) struct CollageArtworkContext<'a> {
    /// Slot list view for determining visible slots
    pub slot_list: &'a SlotListView,
    /// Optional disk cache for mini artwork
    pub disk_cache: Option<&'a DiskCache>,
    /// IDs of items currently being loaded (prevents duplicate requests)
    pub pending_ids: &'a HashSet<String>,
    /// In-memory mini artwork cache
    pub memory_artwork: &'a HashMap<String, image::Handle>,
    /// In-memory collage artwork cache
    pub memory_collage: &'a HashMap<String, Vec<image::Handle>>,
}

/// Result of checking disk cache for an item
#[derive(Debug)]
pub(crate) enum CacheCheckResult {
    /// Both mini and collage already in memory - skip
    FullyCached,
    /// Mini loaded from disk, but need network for collage
    MiniCached(image::Handle),
    /// Need full network load
    NeedNetwork,
    /// Already pending - skip
    AlreadyPending,
}

/// Check disk cache for a collage item
///
/// # Arguments
/// * `item` - The item to check cache for
/// * `ctx` - Context containing cache references
/// * `cache_size` - Size suffix for cache key (e.g., 300 for "id_albumid_300")
pub(crate) fn check_cache<T: CollageArtworkItem>(
    item: &T,
    ctx: &CollageArtworkContext,
    cache_size: u32,
) -> CacheCheckResult {
    let id = item.id();

    // Skip if already pending
    if ctx.pending_ids.contains(id) {
        return CacheCheckResult::AlreadyPending;
    }

    // Skip if both mini and collage already in memory
    if ctx.memory_artwork.contains_key(id) && ctx.memory_collage.contains_key(id) {
        return CacheCheckResult::FullyCached;
    }

    // Try disk cache for mini artwork
    let first_album_id = item.artwork_album_ids().first();
    if let Some(album_id) = first_album_id
        && let Some(cache) = ctx.disk_cache
    {
        let cache_key = format!("{id}_{album_id}_{cache_size}");
        if cache.contains(&cache_key) {
            // Disk cache hit - but check if we still need collage
            if ctx.memory_collage.contains_key(id) {
                // Both mini (from disk) and collage already loaded
                return CacheCheckResult::FullyCached;
            }
            // Mini cached but no collage yet - need network for collage
            return CacheCheckResult::MiniCached(image::Handle::from_path(
                cache.get_path(&cache_key),
            ));
        }
    }

    CacheCheckResult::NeedNetwork
}

/// Generate tasks to load visible collage artwork for all 9 slot list slots
///
/// This handles the core LoadArtwork action pattern shared by genres and playlists.
///
/// # Returns
/// A tuple of:
/// * `pending_inserts` - Item IDs to mark as pending
/// * `cache_inserts` - (id, handle) pairs to insert from disk cache
/// * `tasks` - Network fetch tasks to execute
///
/// # Arguments
/// * `items` - Full list of items
/// * `ctx` - Context with cache/state references
/// * `cache_size` - Size suffix for cache keys
/// * `auth_vm` - Auth view model for fetching credentials
/// * `create_message` - Closure to create the appropriate Message variant
pub(crate) fn load_visible_artwork<T, F>(
    items: &[T],
    ctx: &CollageArtworkContext,
    cache_size: u32,
    auth_vm: AuthGateway,
    create_message: F,
) -> LoadArtworkResult
where
    T: CollageArtworkItem,
    F: Fn(String, String, String, Vec<String>) -> Message + Clone + Send + 'static,
{
    let total = items.len();
    if total == 0 {
        return (Vec::new(), Vec::new(), Vec::new());
    }

    let mut pending_inserts: Vec<String> = Vec::new();
    let mut cache_inserts: Vec<(String, image::Handle)> = Vec::new();
    let mut tasks: Vec<Task<Message>> = Vec::new();

    // Get indices for all visible slot list slots + nearby items for prefetch
    let indices_to_load: Vec<usize> = ctx.slot_list.prefetch_indices(total).collect();

    for idx in indices_to_load {
        if let Some(item) = items.get(idx) {
            match check_cache(item, ctx, cache_size) {
                CacheCheckResult::FullyCached | CacheCheckResult::AlreadyPending => {
                    // Skip - nothing to do
                    continue;
                }
                CacheCheckResult::MiniCached(handle) => {
                    // Insert the cached mini artwork, but still need network for collage
                    let id = item.id().to_string();
                    cache_inserts.push((id.clone(), handle));
                    pending_inserts.push(id.clone());

                    // Create network task for collage
                    let auth_vm_clone = auth_vm.clone();
                    let album_ids = item.artwork_album_ids().to_vec();
                    let create_msg = create_message.clone();
                    tasks.push(Task::perform(
                        async move {
                            let server_url = auth_vm_clone.get_server_url().await;
                            let subsonic_credential = auth_vm_clone.get_subsonic_credential().await;
                            (id, server_url, subsonic_credential, album_ids)
                        },
                        move |(id, url, cred, album_ids)| create_msg(id, url, cred, album_ids),
                    ));
                }
                CacheCheckResult::NeedNetwork => {
                    // Full network load needed
                    let id = item.id().to_string();
                    pending_inserts.push(id.clone());

                    let auth_vm_clone = auth_vm.clone();
                    let album_ids = item.artwork_album_ids().to_vec();
                    let create_msg = create_message.clone();
                    tasks.push(Task::perform(
                        async move {
                            let server_url = auth_vm_clone.get_server_url().await;
                            let subsonic_credential = auth_vm_clone.get_subsonic_credential().await;
                            (id, server_url, subsonic_credential, album_ids)
                        },
                        move |(id, url, cred, album_ids)| create_msg(id, url, cred, album_ids),
                    ));
                }
            }
        }
    }

    trace!(
        "Collage artwork: {} tasks, {} cache inserts, {} pending",
        tasks.len(),
        cache_inserts.len(),
        pending_inserts.len()
    );

    (pending_inserts, cache_inserts, tasks)
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
            let server_url = auth_vm.get_server_url().await;
            let subsonic_credential = auth_vm.get_subsonic_credential().await;
            (ids_to_load, server_url, subsonic_credential)
        },
        move |(ids, url, cred)| create_batch_message(ids, url, cred),
    );

    (pending_inserts, Some(task))
}
