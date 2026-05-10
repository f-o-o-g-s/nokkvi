//! Shared collage artwork result handlers and loading helpers (genre / playlist).
//!
//! The result-handling side (`handle_collage_*_loaded`) was already unified.
//! This module also provides the _loading_ side so that `genres.rs` and
//! `playlists.rs` only need thin wrappers that supply the entity-specific
//! album-ID fetch closure.

use std::future::Future;

use iced::{Task, widget::image};

use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, CollageTarget, Message},
    state::CollageArtworkCache,
};

impl Nokkvi {
    /// Returns a mutable reference to the collage artwork cache for the given target.
    pub(crate) fn collage_cache_mut(&mut self, target: CollageTarget) -> &mut CollageArtworkCache {
        match target {
            CollageTarget::Genre => &mut self.artwork.genre,
            CollageTarget::Playlist => &mut self.artwork.playlist,
        }
    }

    /// Unified collage artwork loader for both genres and playlists.
    ///
    /// # Parameters
    /// - `target`: Genre or Playlist — determines which disk cache and message variant to use
    /// - `entity_id`: The genre/playlist ID to load artwork for
    /// - `server_url`, `subsonic_credential`: API auth context
    /// - `cached_album_ids`: Pre-resolved album IDs (empty → fetch via `fetch_album_ids_fn`)
    /// - `fetch_album_ids_fn`: Closure that fetches album IDs from the appropriate API service
    ///   when `cached_album_ids` is empty
    pub(crate) fn handle_load_collage_artwork<F, Fut>(
        &mut self,
        target: CollageTarget,
        entity_id: String,
        server_url: String,
        subsonic_credential: String,
        cached_album_ids: Vec<String>,
        fetch_album_ids_fn: F,
    ) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::services::api::client::ApiClient, String, String, String) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = Vec<String>> + Send,
    {
        let entity_id_clone = entity_id.clone();
        let artwork_size = self.artwork_resolution.to_size();

        self.shell_task(
            move |shell| async move {
                let auth_vm = shell.auth().clone();
                let albums_vm = shell.albums().clone();

                // Use cached album IDs if available, otherwise fetch from API
                let album_ids = if !cached_album_ids.is_empty() {
                    cached_album_ids
                } else {
                    let client = match auth_vm.get_client().await {
                        Some(c) => c,
                        None => return (entity_id_clone, None, Vec::new(), Vec::new()),
                    };
                    fetch_album_ids_fn(
                        client,
                        server_url.clone(),
                        subsonic_credential.clone(),
                        entity_id_clone.clone(),
                    )
                    .await
                };

                if album_ids.is_empty() {
                    return (entity_id_clone, None, Vec::new(), Vec::new());
                }

                // 1. Load mini artwork (first album) at 300px via the cached
                //    client. Use the retry variant so a single 429 from
                //    Navidrome's throttle (e.g. when a scrollbar-drag settle
                //    fans out ~10× per visible item) doesn't leave the slot
                //    permanently blank — see `fetch_album_artwork_with_retry`.
                let first_album_id = album_ids[0].clone();
                let mini_vm = albums_vm.clone();
                let mini_handle_fut = async move {
                    mini_vm
                        .fetch_album_artwork_with_retry(&first_album_id, Some(300), None)
                        .await
                        .ok()
                        .map(image::Handle::from_bytes)
                };

                // 2. Single-album special case: full-res artwork as the sole tile.
                if album_ids.len() == 1 {
                    let full_size =
                        artwork_size.or(Some(nokkvi_data::utils::artwork_url::HIGH_RES_SIZE));
                    let full_vm = albums_vm.clone();
                    let only_id = album_ids[0].clone();
                    let (mini_handle, full_res_bytes) =
                        futures::join!(mini_handle_fut, async move {
                            full_vm
                                .fetch_album_artwork_with_retry(&only_id, full_size, None)
                                .await
                                .ok()
                        });

                    let mut collage_handles = Vec::new();
                    if let Some(bytes) = full_res_bytes {
                        collage_handles.push(image::Handle::from_bytes(bytes));
                    }

                    return (entity_id_clone, mini_handle, collage_handles, album_ids);
                }

                // Multiple albums: fetch up to 9 tiles at 300px in parallel.
                // Same retry rationale as the mini fetch above — burst settle
                // from one drag can dispatch ~117 tile requests at once.
                let collage_tiles_futs: Vec<_> = album_ids
                    .iter()
                    .take(9)
                    .cloned()
                    .map(|id| {
                        let vm = albums_vm.clone();
                        async move {
                            vm.fetch_album_artwork_with_retry(&id, Some(300), None)
                                .await
                                .ok()
                        }
                    })
                    .collect();

                let (mini_handle, collage_results) = futures::join!(
                    mini_handle_fut,
                    futures::future::join_all(collage_tiles_futs)
                );

                let collage_handles: Vec<_> = collage_results
                    .into_iter()
                    .flatten()
                    .map(image::Handle::from_bytes)
                    .collect();

                (entity_id_clone, mini_handle, collage_handles, album_ids)
            },
            move |(entity_id, mini_handle, collage_handles, album_ids)| {
                Message::Artwork(ArtworkMessage::CollageLoaded(
                    target,
                    entity_id,
                    mini_handle,
                    collage_handles,
                    album_ids,
                ))
            },
        )
    }

    /// Unified album-ID prefetch for collage artwork (genres and playlists).
    ///
    /// Collects items that still need album IDs (`artwork_album_ids` is empty),
    /// fetches them in parallel via the supplied closure, and emits
    /// `CollageAlbumIdsLoaded` when done. Once stored on the library items,
    /// these IDs let the viewport-driven `LoadArtwork` path skip a per-item
    /// album-list roundtrip; mini/collage artwork itself is loaded lazily by
    /// that viewport-driven path, not eagerly here, so the bounded mini LRU
    /// can't be thrashed by a serial fetch over every genre/playlist.
    pub(crate) fn handle_start_collage_prefetch<F, Fut>(
        &mut self,
        target: CollageTarget,
        items_needing_ids: Vec<(String, String)>,
        fetch_album_ids_fn: F,
    ) -> Task<Message>
    where
        F: Fn(nokkvi_data::services::api::client::ApiClient, String, String, String) -> Fut
            + Send
            + 'static,
        Fut: Future<Output = Vec<String>> + Send,
    {
        if items_needing_ids.is_empty() {
            return Task::none();
        }

        tracing::debug!(
            " Starting collage prefetch for {} {:?} items",
            items_needing_ids.len(),
            target
        );

        // Fetch album IDs for all items in parallel
        self.shell_task(
            move |shell| async move {
                let auth_vm = shell.auth().clone();
                let (server_url, subsonic_credential) = {
                    let url = auth_vm.get_server_url().await;
                    let cred = auth_vm.get_subsonic_credential().await;
                    (url, cred)
                };

                let client = match auth_vm.get_client().await {
                    Some(c) => c,
                    None => return Vec::new(),
                };

                let futures: Vec<_> = items_needing_ids
                    .into_iter()
                    .map(|(item_id, _name)| {
                        let server_url = server_url.clone();
                        let subsonic_credential = subsonic_credential.clone();
                        let client = client.clone();

                        let fetch = &fetch_album_ids_fn;
                        let fut = fetch(client, server_url, subsonic_credential, item_id.clone());

                        async move {
                            let album_ids = fut.await;
                            (item_id, album_ids)
                        }
                    })
                    .collect();

                futures::future::join_all(futures).await
            },
            move |results: Vec<(String, Vec<String>)>| {
                Message::Artwork(ArtworkMessage::CollageAlbumIdsLoaded(target, results))
            },
        )
    }

    /// Updates artwork_album_ids for the item matching `item_id` in the correct list.
    fn set_collage_item_album_ids(
        &mut self,
        target: CollageTarget,
        item_id: &str,
        album_ids: Vec<String>,
    ) {
        match target {
            CollageTarget::Genre => {
                if let Some(g) = self.library.genres.iter_mut().find(|g| g.id == item_id) {
                    g.artwork_album_ids = album_ids;
                }
            }
            CollageTarget::Playlist => {
                if let Some(p) = self.library.playlists.iter_mut().find(|p| p.id == item_id) {
                    p.artwork_album_ids = album_ids;
                }
            }
        }
    }

    // ---- Unified result handlers ----

    /// Handle a mini (single-album) artwork load result for a collage target.
    pub(crate) fn handle_collage_mini_loaded(
        &mut self,
        target: CollageTarget,
        item_id: String,
        handle_opt: Option<image::Handle>,
    ) -> Task<Message> {
        let cache = self.collage_cache_mut(target);
        cache.pending.remove(&item_id);
        if let Some(handle) = handle_opt {
            cache.mini.put(item_id, handle);
            cache.refresh_snapshot();
        }
        Task::none()
    }

    /// Handle a full collage artwork load result (mini + collage tiles + album IDs).
    pub(crate) fn handle_collage_artwork_loaded(
        &mut self,
        target: CollageTarget,
        item_id: String,
        handle_opt: Option<image::Handle>,
        collage_handles: Vec<image::Handle>,
        album_ids: Vec<String>,
    ) -> Task<Message> {
        let cache = self.collage_cache_mut(target);
        cache.pending.remove(&item_id);

        let mut mutated = false;
        if let Some(handle) = handle_opt {
            cache.mini.put(item_id.clone(), handle);
            mutated = true;
        }
        if !collage_handles.is_empty() {
            cache.collage.put(item_id.clone(), collage_handles);
            mutated = true;
        }
        if mutated {
            cache.refresh_snapshot();
        }

        if !album_ids.is_empty() {
            self.set_collage_item_album_ids(target, &item_id, album_ids);
        }
        Task::none()
    }

    /// Handle album IDs resolution result — store IDs on items so the
    /// viewport-driven `LoadArtwork` path can skip a per-item album-list
    /// roundtrip when scrolling fetches its mini/collage artwork.
    pub(crate) fn handle_collage_album_ids_loaded(
        &mut self,
        target: CollageTarget,
        results: Vec<(String, Vec<String>)>,
    ) -> Task<Message> {
        for (item_id, album_ids) in results {
            self.set_collage_item_album_ids(target, &item_id, album_ids);
        }
        Task::none()
    }
}
