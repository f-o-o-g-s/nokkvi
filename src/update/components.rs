//! Shared component handler utilities and canonical artwork prefetch helpers.
//!
//! This module contains only shared helpers used by the per-view handler files
//! (`albums.rs`, `artists.rs`, `songs.rs`, `genres.rs`, `playlists.rs`, `queue.rs`, `player_bar.rs`).
//!
//! ## Handler Invariants
//!
//! These helpers translate bubbled-up page actions into app-level Tasks.
//! They must remain thin routing layers:
//!
//! - **No viewport math** — use `slot_list.indices_to_prefetch()` or `slot_list.get_center_item_index()`
//! - **No async orchestration** — delegate to `AppService` methods
//! - **No cache inspection loops** — use `prefetch_album_artwork_tasks()` or similar helpers
//! - **Handlers schedule tasks only** — they don't define "how", just "what"
//!
//! If you find yourself writing a `for` loop or an `async move { ... }` block with
//! multi-step logic, stop and extract it to the appropriate layer:
//! - Viewport logic → `SlotListView`
//! - Playback orchestration → `AppService`
//! - Artwork prefetching → helper functions in this module
use std::collections::HashSet;

use iced::{Task, widget::image};
use nokkvi_data::backend::albums::AlbumsService;
use tracing::{debug, error, info};

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, HotkeyMessage, Message},
    views,
    widgets::SlotListView,
};

/// Result type for the combined "resolve song IDs + fetch playlist list" async task.
/// Pairs: `(playlist_id, playlist_name)` list + resolved song IDs.
type PlaylistSongResolveResult = Result<(Vec<(String, String)>, Vec<String>), anyhow::Error>;

/// Generate artwork prefetch tasks for a slot list viewport.
///
/// This is the single authoritative implementation of artwork prefetching.
/// All slot-list-based views should use this instead of inline loops.
pub(super) fn prefetch_album_artwork_tasks<F, T>(
    slot_list: &SlotListView,
    items: &[T],
    cached_ids: &HashSet<&String>,
    albums_vm: AlbumsService,
    extract_id_url: F,
) -> Vec<Task<Message>>
where
    F: Fn(&T) -> (String, String),
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
            let (id, url) = extract_id_url(item);
            // Skip if cached or already queued in this batch
            if cached_ids.contains(&id) || already_queued.contains(&id) {
                None
            } else {
                already_queued.insert(id.clone());
                Some((id, url))
            }
        })
        .map(|(id, url)| {
            let vm = albums_vm.clone();
            Task::perform(
                async move {
                    let path = vm.get_artwork_cache_path(&url, Some(80)).await;
                    (id, path.map(image::Handle::from_path))
                },
                |(id, handle)| Message::Artwork(ArtworkMessage::Loaded(id, handle)),
            )
        })
        .collect()
}

/// Generate song artwork prefetch tasks for a slot list viewport.
///
/// Variant of `prefetch_album_artwork_tasks` for songs that have `Option<album_id>`.
/// Uses `Message::SongArtworkLoaded` instead of `Message::ArtworkLoaded`.
pub(super) fn prefetch_song_artwork_tasks(
    slot_list: &SlotListView,
    songs: &[nokkvi_data::backend::songs::SongUIViewData],
    cached_ids: &HashSet<&String>,
    albums_vm: AlbumsService,
) -> Vec<Task<Message>> {
    let total = songs.len();
    if total == 0 {
        return Vec::new();
    }

    let mut already_queued = HashSet::new();

    slot_list
        .prefetch_indices(total)
        .filter_map(|idx| songs.get(idx))
        .filter_map(|song| {
            song.album_id.as_ref().and_then(|id| {
                if cached_ids.contains(id) || already_queued.contains(id) {
                    None
                } else {
                    already_queued.insert(id.clone());
                    Some(id.clone())
                }
            })
        })
        .map(|album_id| {
            let vm = albums_vm.clone();
            let id = album_id.clone();
            Task::perform(
                async move {
                    let (url, cred) = vm.get_server_config().await;
                    let artwork_url = nokkvi_data::utils::artwork_url::build_cover_art_url(
                        &id,
                        &url,
                        &cred,
                        Some(80),
                    );
                    let path = vm.get_artwork_cache_path(&artwork_url, Some(80)).await;
                    (id, path.map(image::Handle::from_path))
                },
                |(id, handle)| {
                    Message::Artwork(crate::app_message::ArtworkMessage::SongMiniLoaded(
                        id, handle,
                    ))
                },
            )
        })
        .collect()
}

impl Nokkvi {
    // ── Helper methods for deduplicating component handler patterns ──

    /// Play SFX for view-level message events.
    ///
    /// Call at the top of each `handle_*` function with two booleans indicating
    /// whether the incoming message is a slot list-navigation or expand/collapse event.
    pub(crate) fn play_view_sfx(&self, is_nav: bool, is_expand: bool) {
        if is_nav {
            self.sfx_engine.play(nokkvi_data::audio::SfxType::Tab);
        }
        if is_expand {
            self.sfx_engine
                .play(nokkvi_data::audio::SfxType::ExpandCollapse);
        }
    }

    /// Guard a play action against playlist edit mode.
    ///
    /// Call this after the browsing panel check in every Play* handler.
    /// Returns `Some(Task::none())` with a warning toast if in playlist
    /// edit mode (play would replace the queue being edited).
    /// Returns `None` and clears `active_playlist_name` if play should proceed.
    pub(crate) fn guard_play_action(&mut self) -> Option<Task<Message>> {
        if self.playlist_edit.is_some() {
            self.toast_warn("Cannot play — would replace the playlist being edited");
            return Some(Task::none());
        }
        // Cancel any in-progress progressive queue loading target so the header
        // doesn't show a stale "X of Y" count from a superseded play action.
        self.library.queue_loading_target = None;
        self.active_playlist_info = None;
        None
    }

    /// Play an entity by parsing an index string, looking up the item, and calling a shell method.
    /// Used by albums, artists, genres, playlists (all follow: parse index → get ID → shell → SwitchView).
    pub(crate) fn play_entity_task<T, F, Fut>(
        &self,
        items: &[T],
        index_str: &str,
        entity_name: &'static str,
        get_id: impl FnOnce(&T) -> String,
        play_fn: F,
    ) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::backend::app_service::AppService, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        if let Ok(index) = index_str.parse::<usize>()
            && let Some(item) = items.get(index)
        {
            let id = get_id(item);
            debug!(" Playing {}: index {}", entity_name, index);
            return self.shell_task(
                move |shell| async move { play_fn(shell, id).await },
                move |result| match result {
                    Ok(()) => Message::SwitchView(View::Queue),
                    Err(e) => {
                        error!(" Failed to play {}: {}", entity_name, e);
                        Message::Toast(crate::app_message::ToastMessage::Push(
                            nokkvi_data::types::toast::Toast::new(
                                format!("Failed to play {entity_name}: {e}"),
                                nokkvi_data::types::toast::ToastLevel::Error,
                            ),
                        ))
                    }
                },
            );
        }
        Task::none()
    }

    /// Add an entity to queue by parsing an index string, looking up the item, and calling a shell method.
    /// Used by albums, artists, genres, playlists (all follow: parse index → get ID → shell → LoadQueue).
    pub(crate) fn add_entity_to_queue_task<T, F, Fut>(
        &self,
        items: &[T],
        index_str: &str,
        entity_name: &'static str,
        get_id: impl FnOnce(&T) -> String,
        get_name: impl FnOnce(&T) -> String,
        add_fn: F,
    ) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::backend::app_service::AppService, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        if let Ok(index) = index_str.parse::<usize>()
            && let Some(item) = items.get(index)
        {
            let id = get_id(item);
            let name = get_name(item);
            debug!(
                " Adding {} '{}' to queue: index {}",
                entity_name, name, index
            );
            return self.shell_task(
                move |shell| async move { add_fn(shell, id).await },
                move |result| match result {
                    Ok(()) => {
                        info!(" Added {} '{}' to queue", entity_name, name);
                        Message::Toast(crate::app_message::ToastMessage::PushThen(
                            nokkvi_data::types::toast::Toast::new(
                                format!("Added '{name}' to queue"),
                                nokkvi_data::types::toast::ToastLevel::Success,
                            ),
                            Box::new(Message::LoadQueue),
                        ))
                    }
                    Err(e) => {
                        error!(" Failed to add {} to queue: {}", entity_name, e);
                        Message::Toast(crate::app_message::ToastMessage::Push(
                            nokkvi_data::types::toast::Toast::new(
                                format!("Failed to add {entity_name} to queue: {e}"),
                                nokkvi_data::types::toast::ToastLevel::Error,
                            ),
                        ))
                    }
                },
            );
        }
        Task::none()
    }

    /// Insert an entity into the queue at a specific position.
    /// Same as `add_entity_to_queue_task` but inserts at `position` instead of appending.
    /// Used when a cross-pane drag drop targets a specific queue slot.
    #[expect(clippy::too_many_arguments)] // Mirrors add_entity_to_queue_task (7 args) +1 position; generics make struct awkward
    pub(crate) fn insert_entity_to_queue_at_position_task<T, F, Fut>(
        &self,
        items: &[T],
        index_str: &str,
        entity_name: &'static str,
        position: usize,
        get_id: impl FnOnce(&T) -> String,
        get_name: impl FnOnce(&T) -> String,
        insert_fn: F,
    ) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::backend::app_service::AppService, String, usize) -> Fut
            + Send
            + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        if let Ok(index) = index_str.parse::<usize>()
            && let Some(item) = items.get(index)
        {
            let id = get_id(item);
            let name = get_name(item);
            debug!(
                " Inserting {} '{}' at queue position {}: index {}",
                entity_name, name, position, index
            );
            return self.shell_task(
                move |shell| async move { insert_fn(shell, id, position).await },
                move |result| match result {
                    Ok(()) => {
                        info!(
                            " Inserted {} '{}' at queue position {}",
                            entity_name, name, position
                        );
                        Message::Toast(crate::app_message::ToastMessage::PushThen(
                            nokkvi_data::types::toast::Toast::new(
                                format!("Inserted '{name}' at position {}", position + 1),
                                nokkvi_data::types::toast::ToastLevel::Success,
                            ),
                            Box::new(Message::LoadQueue),
                        ))
                    }
                    Err(e) => {
                        error!(" Failed to insert {} to queue: {}", entity_name, e);
                        Message::Toast(crate::app_message::ToastMessage::Push(
                            nokkvi_data::types::toast::Toast::new(
                                format!("Failed to add {entity_name} to queue: {e}"),
                                nokkvi_data::types::toast::ToastLevel::Error,
                            ),
                        ))
                    }
                },
            );
        }
        Task::none()
    }

    /// Persist view preferences (sort mode + sort order) and return a load task.
    /// Used by albums, artists, songs, genres, playlists for both SortModeChanged and SortOrderChanged.
    pub(crate) fn persist_view_prefs<F, Fut>(
        &self,
        task_name: &'static str,
        sort_mode: crate::widgets::view_header::SortMode,
        ascending: bool,
        load_msg: Message,
        persist_fn: F,
    ) -> Task<Message>
    where
        F: FnOnce(
                nokkvi_data::backend::app_service::AppService,
                crate::widgets::view_header::SortMode,
                bool,
            ) -> Fut
            + Send
            + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        self.shell_spawn(task_name, move |shell| async move {
            persist_fn(shell, sort_mode, ascending).await
        });
        Task::done(load_msg)
    }

    /// Execute an async action on the shell, mapping success to `success_msg` and failure to `NoOp`.
    /// This is the foundational primitive — most shell-based actions route through here.
    ///
    /// # Usage
    /// ```ignore
    /// self.shell_action_task(
    ///     move |shell| async move { shell.play_genre(&name).await },
    ///     Message::SwitchView(View::Queue),
    ///     "play genre",
    /// )
    /// ```
    pub(crate) fn shell_action_task<F, Fut>(
        &self,
        action_fn: F,
        success_msg: Message,
        error_ctx: &'static str,
    ) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::backend::app_service::AppService) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        self.shell_task(action_fn, move |result| match result {
            Ok(()) => success_msg,
            Err(e) => {
                error!(" Failed to {}: {}", error_ctx, e);
                Message::Toast(crate::app_message::ToastMessage::Push(
                    nokkvi_data::types::toast::Toast::new(
                        format!("Failed to {error_ctx}: {e}"),
                        nokkvi_data::types::toast::ToastLevel::Error,
                    ),
                ))
            }
        })
    }

    /// Execute an async action on the shell, showing a success toast and reloading
    /// the queue on success. Used for add-to-queue operations from expanded views.
    pub(crate) fn shell_fire_and_forget_task<F, Fut>(
        &self,
        action_fn: F,
        success_label: String,
        error_ctx: &'static str,
    ) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::backend::app_service::AppService) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        self.shell_task(action_fn, move |result| match result {
            Ok(()) => {
                info!(" {}", success_label);
                Message::Toast(crate::app_message::ToastMessage::PushThen(
                    nokkvi_data::types::toast::Toast::new(
                        success_label,
                        nokkvi_data::types::toast::ToastLevel::Success,
                    ),
                    Box::new(Message::LoadQueue),
                ))
            }
            Err(e) => {
                error!(" Failed to {}: {}", error_ctx, e);
                Message::Toast(crate::app_message::ToastMessage::Push(
                    nokkvi_data::types::toast::Toast::new(
                        format!("Failed to {error_ctx}: {e}"),
                        nokkvi_data::types::toast::ToastLevel::Error,
                    ),
                ))
            }
        })
    }

    /// Handle the common view actions (SearchChanged, SortModeChanged, SortOrderChanged, None)
    /// that are identical across all 5 non-Queue views.
    ///
    /// Returns `Some(task)` if the action was handled, `None` if it's view-specific
    /// and the caller should continue with its own match arms.
    pub(crate) fn handle_common_view_action<F, Fut>(
        &self,
        common_action: views::CommonViewAction,
        reload_msg: Message,
        persist_name: &'static str,
        sort_mode: crate::widgets::view_header::SortMode,
        sort_ascending: bool,
        persist_fn: F,
    ) -> Option<Task<Message>>
    where
        F: FnOnce(
                nokkvi_data::backend::app_service::AppService,
                crate::widgets::view_header::SortMode,
                bool,
            ) -> Fut
            + Send
            + 'static
            + Clone,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        match common_action {
            views::CommonViewAction::SearchChanged => Some(Task::done(reload_msg)),
            views::CommonViewAction::SortModeChanged(new_sort_mode) => {
                let pf = persist_fn;
                Some(self.persist_view_prefs(
                    persist_name,
                    new_sort_mode,
                    sort_ascending,
                    reload_msg,
                    pf,
                ))
            }
            views::CommonViewAction::SortOrderChanged(ascending) => {
                let pf = persist_fn;
                Some(self.persist_view_prefs(persist_name, sort_mode, ascending, reload_msg, pf))
            }
            views::CommonViewAction::None | views::CommonViewAction::ViewSpecific => None,
        }
    }

    /// Star or unstar an item via the Subsonic API.
    /// Optimistic local state updates should be done inline at the call site before calling this.
    /// On failure, emits a revert message to restore the original starred state.
    pub(crate) fn star_item_task(
        &self,
        id: String,
        item_type: &'static str,
        star: bool,
    ) -> Task<Message> {
        let action = if star { "star" } else { "unstar" };
        let revert_id = id.clone();
        debug!(
            " {} {} {}",
            if star { "Starring" } else { "Unstarring" },
            item_type,
            id
        );
        self.shell_task(
            move |shell| async move {
                let auth_vm = shell.auth().clone();
                let client = auth_vm
                    .get_client()
                    .await
                    .ok_or_else(|| anyhow::anyhow!("No API client available"))?;
                let server_url = auth_vm.get_server_url().await;
                let subsonic_credential = auth_vm.get_subsonic_credential().await;
                if star {
                    nokkvi_data::services::api::star::star_item(
                        &client.http_client(),
                        &server_url,
                        &subsonic_credential,
                        &id,
                        item_type,
                    )
                    .await
                } else {
                    nokkvi_data::services::api::star::unstar_item(
                        &client.http_client(),
                        &server_url,
                        &subsonic_credential,
                        &id,
                        item_type,
                    )
                    .await
                }
            },
            move |result| {
                if let Err(e) = result {
                    error!(" Failed to {} {}: {}", action, item_type, e);
                    // Revert optimistic update by emitting the original starred state
                    return Self::starred_revert_message(revert_id, item_type, !star);
                }
                Message::NoOp
            },
        )
    }

    /// Build the appropriate starred-status-updated message for a given item type.
    /// Used to revert optimistic star updates on API failure.
    pub(crate) fn starred_revert_message(id: String, item_type: &str, starred: bool) -> Message {
        match item_type {
            "album" => Message::Hotkey(HotkeyMessage::AlbumStarredStatusUpdated(id, starred)),
            "artist" => Message::Hotkey(HotkeyMessage::ArtistStarredStatusUpdated(id, starred)),
            _ => Message::Hotkey(HotkeyMessage::SongStarredStatusUpdated(id, starred)),
        }
    }

    /// Build the appropriate rating-updated message for a given item type.
    /// Used to revert optimistic rating updates on API failure.
    pub(crate) fn rating_revert_message(id: String, item_type: &str, rating: u32) -> Message {
        match item_type {
            "album" => Message::Hotkey(HotkeyMessage::AlbumRatingUpdated(id, rating)),
            "artist" => Message::Hotkey(HotkeyMessage::ArtistRatingUpdated(id, rating)),
            _ => Message::Hotkey(HotkeyMessage::SongRatingUpdated(id, rating)),
        }
    }

    /// Set an absolute rating on an item via the Subsonic API.
    /// Applies optimistic UI update immediately, reverts on failure.
    pub(crate) fn set_item_rating_task(
        &self,
        id: String,
        item_type: &str,
        new_rating: usize,
        current_rating: u32,
    ) -> Task<Message> {
        let new_rating_u32 = new_rating as u32;
        debug!(
            "⭐ Setting rating for {} {}: {} -> {}",
            item_type, id, current_rating, new_rating
        );

        // Optimistic update
        let optimistic_msg = Self::rating_revert_message(id.clone(), item_type, new_rating_u32);

        let revert_id = id.clone();
        let revert_type = item_type.to_string();
        let item_id = id;

        let api_task = self.shell_task(
            move |shell| async move {
                let auth_vm = shell.auth().clone();
                let client = auth_vm
                    .get_client()
                    .await
                    .ok_or_else(|| anyhow::anyhow!("No API client available"))?;
                let server_url = auth_vm.get_server_url().await;
                let subsonic_credential = auth_vm.get_subsonic_credential().await;

                nokkvi_data::services::api::rating::set_rating(
                    &client.http_client(),
                    &server_url,
                    &subsonic_credential,
                    &item_id,
                    new_rating as u32,
                )
                .await?;

                Ok::<_, anyhow::Error>(())
            },
            move |result| match result {
                Ok(()) => Message::NoOp,
                Err(e) => {
                    error!(" Failed to set rating: {}", e);
                    Self::rating_revert_message(revert_id, &revert_type, current_rating)
                }
            },
        );

        Task::batch(vec![Task::done(optimistic_msg), api_task])
    }

    /// Resolve song IDs for an entity, fetch the playlists list, and open the dialog.
    ///
    /// This is the single-point-of-truth for the "resolve songs → fetch playlists → map result"
    /// pattern used by albums, artists, genres, and playlists `AddToPlaylist` handlers.
    ///
    /// `resolve_fn` should return the `Vec<String>` of song IDs for the entity.
    pub(crate) fn resolve_and_add_to_playlist<F, Fut>(
        &self,
        resolve_fn: F,
        error_ctx: &'static str,
    ) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::backend::app_service::AppService) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<Vec<String>>> + Send,
    {
        self.shell_task(
            move |shell| async move {
                let song_ids = resolve_fn(shell.clone()).await?;
                let service = shell.playlists_api().await?;
                let (playlists, _) = service.load_playlists("name", "ASC", None).await?;
                let playlist_pairs: Vec<(String, String)> =
                    playlists.into_iter().map(|p| (p.id, p.name)).collect();
                Ok((playlist_pairs, song_ids))
            },
            |result| Self::map_add_to_playlist_result(result, error_ctx),
        )
    }

    /// Map the result of a combined "resolve songs + fetch playlists" async task
    /// into the appropriate `Message`.
    ///
    /// Eliminates the repeated complex `Result<(Vec<(String, String)>, Vec<String>), Error>`
    /// closure annotations across albums/artists/genres handlers.
    pub(crate) fn map_add_to_playlist_result(
        result: PlaylistSongResolveResult,
        error_ctx: &str,
    ) -> Message {
        match result {
            Ok((playlists, song_ids)) => {
                Message::PlaylistsFetchedForAddToPlaylist(playlists, song_ids)
            }
            Err(e) => {
                tracing::error!("Failed to {error_ctx}: {e}");
                Message::Toast(crate::app_message::ToastMessage::Push(
                    nokkvi_data::types::toast::Toast::new(
                        format!("Failed to load data: {e}"),
                        nokkvi_data::types::toast::ToastLevel::Error,
                    ),
                ))
            }
        }
    }

    /// Open the containing folder of a song file in the user's file manager.
    ///
    /// `relative_path` is the song's path as stored by Navidrome (relative to the
    /// music library root). The method prepends `self.local_music_path`, resolves
    /// the parent directory, and opens it with `xdg-open`.
    pub(crate) fn handle_show_in_folder(&mut self, relative_path: String) -> Task<Message> {
        if self.local_music_path.is_empty() {
            self.toast_warn(
                "Set a Local Music Path in Settings → Application to open files in your file manager.",
            );
            return Task::none();
        }

        let prefix = self.local_music_path.trim_end_matches('/');
        let full_path = format!("{prefix}/{relative_path}");
        let file_path = std::path::Path::new(&full_path);

        // Open the parent directory so the file manager shows the folder
        let folder = file_path
            .parent()
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        if !std::path::Path::new(&folder).exists() {
            self.toast_warn(format!(
                "Path not found: {folder}\nCheck your Local Music Path in Settings."
            ));
        } else if let Err(e) = std::process::Command::new("xdg-open").arg(&folder).spawn() {
            tracing::warn!("Failed to open folder '{}': {}", folder, e);
            self.toast_warn(format!("Could not open file manager: {e}"));
        }

        Task::none()
    }

    // ── Strip context menu helpers ──────────────────────────────────────

    /// Whether the currently playing track is starred.
    /// Used to render the star/unstar label in the strip context menu.
    pub(crate) fn is_current_track_starred(&self) -> bool {
        let Some(song_id) = &self.scrobble.current_song_id else {
            return false;
        };
        self.library
            .queue_songs
            .iter()
            .find(|s| &s.id == song_id)
            .is_some_and(|s| s.starred)
    }

    /// Toggle star on the currently playing track via the strip context menu.
    /// Uses the existing `star_item_task` pattern with optimistic update.
    pub(crate) fn handle_toggle_star_for_playing_track(&mut self) -> Task<Message> {
        let Some(song_id) = self.scrobble.current_song_id.clone() else {
            self.toast_warn("No track is currently playing");
            return Task::none();
        };

        let is_starred = self.is_current_track_starred();
        let new_starred = !is_starred;
        let name = self.playback.title.clone();

        // Optimistic update
        let optimistic_msg = Self::starred_revert_message(song_id.clone(), "song", new_starred);

        // API call
        let api_task = self.star_item_task(song_id, "song", new_starred);

        // Toast
        let toast_label = if new_starred {
            format!("♥ Loved: {name}")
        } else {
            format!("Unloved: {name}")
        };
        let toast_msg = Message::Toast(crate::app_message::ToastMessage::Push(
            nokkvi_data::types::toast::Toast::new(
                toast_label,
                nokkvi_data::types::toast::ToastLevel::Success,
            ),
        ));

        Task::batch(vec![
            Task::done(optimistic_msg),
            api_task,
            Task::done(toast_msg),
        ])
    }

    /// Open the currently playing track's folder in the file manager.
    /// Uses API lookup since QueueSongUIViewData doesn't carry the file path.
    pub(crate) fn handle_show_in_folder_for_playing_track(&mut self) -> Task<Message> {
        let Some(song_id) = self.scrobble.current_song_id.clone() else {
            self.toast_warn("No track is currently playing");
            return Task::none();
        };

        self.shell_task(
            move |shell| async move {
                let api = shell.songs_api().await?;
                let song = api.load_song_by_id(&song_id).await?;
                Ok(song.path)
            },
            |result: Result<String, anyhow::Error>| match result {
                Ok(path) => Message::ShowInFolder(path),
                Err(e) => {
                    tracing::error!("Failed to load song path: {e}");
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to load song path: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
        )
    }
}
