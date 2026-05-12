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
use nokkvi_data::{
    backend::albums::AlbumsService,
    types::{ItemKind, error::NokkviError},
    utils::artwork_url::THUMBNAIL_SIZE,
};
use tracing::{debug, error, info};

use crate::{
    Nokkvi, View,
    app_message::{ArtworkMessage, HotkeyMessage, Message},
    views,
    widgets::{SlotListPageState, SlotListView, view_header::SortMode},
};

/// Bundled params for a paginated library fetch (Albums, Artists, Songs).
///
/// Built once at the call site from the page's common state via
/// [`PaginatedFetch::from_common`], then moved into the spawned
/// `shell_task` closure. Owned values (no lifetimes) so the struct can
/// cross the `'static` boundary.
pub(crate) struct PaginatedFetch {
    pub view_str: &'static str,
    pub sort_order: &'static str,
    pub search_query: Option<String>,
    pub filter: Option<nokkvi_data::types::filter::LibraryFilter>,
    pub offset: usize,
    pub page_size: usize,
}

impl PaginatedFetch {
    pub(crate) fn from_common(
        common: &SlotListPageState,
        sort_to_api: fn(SortMode) -> &'static str,
        offset: usize,
        page_size: usize,
    ) -> Self {
        let view_str = sort_to_api(common.current_sort_mode);
        let sort_order = if common.sort_ascending { "ASC" } else { "DESC" };
        let search_query = (!common.search_query.is_empty()).then(|| common.search_query.clone());
        let filter = common.active_filter.clone();
        Self {
            view_str,
            sort_order,
            search_query,
            filter,
            offset,
            page_size,
        }
    }
}

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
                    let bytes = vm.fetch_artwork_by_url(&url).await.ok();
                    (id, bytes.map(image::Handle::from_bytes))
                },
                |(id, handle)| Message::Artwork(ArtworkMessage::Loaded(id, handle)),
            )
        })
        .collect()
}

/// Generate song artwork prefetch tasks for a slot list viewport.
///
/// Variant of `prefetch_album_artwork_tasks` for songs that have
/// `Option<album_id>`. Generic over the slice element type — Songs page
/// passes `SongUIViewData`, Similar page passes raw `Song`. The
/// `extract_album_id` closure pulls the optional album id out of each
/// element. Dispatches `Message::Artwork(ArtworkMessage::SongMiniLoaded)`.
pub(super) fn prefetch_song_artwork_tasks<T, F>(
    slot_list: &SlotListView,
    songs: &[T],
    cached_ids: &HashSet<&String>,
    albums_vm: AlbumsService,
    extract_album_id: F,
) -> Vec<Task<Message>>
where
    F: Fn(&T) -> Option<&String>,
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
            extract_album_id(song).and_then(|id| {
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
            let id = album_id;
            Task::perform(
                async move {
                    let bytes = vm
                        .fetch_album_artwork(&id, Some(THUMBNAIL_SIZE), None)
                        .await
                        .ok();
                    (id, bytes.map(image::Handle::from_bytes))
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

/// Fan-out 80px album artwork fetches for albums newly delivered into a
/// view's expansion children (Artists→Album, Genres→Album). Skips ids
/// already in the cache; each surviving id dispatches an
/// `ArtworkMessage::Loaded` so the centralized `handle_artwork_loaded`
/// arm puts the handle into `album_art` exactly the way Albums view does.
///
/// Callers pass `(album.id, album.artwork_url)` pairs — the URL is
/// pre-built by `AlbumUIViewData::from_album` from `album.cover_art`
/// (with `album.id` as fallback). For albums whose artwork lives on a
/// media file (`cover_art = "mf-…"`) this matters — passing only the
/// album id would build the wrong URL and the fetch would return empty.
///
/// Each fetch goes through `fetch_artwork_by_url_with_retry` (3 attempts,
/// 100 ms / 200 ms backoff). Without retries, large expansions (e.g. a
/// genre with 150+ albums) reliably drop 1–2 thumbnails because
/// Navidrome's `getCoverArt` throttle middleware rejects requests that
/// exceed its in-flight backlog cap. Genre/artist expansions have no
/// scroll-triggered re-fetch path, so a single dropped fetch leaves a
/// permanently-blank slot until the next expansion.
pub(super) fn expansion_album_artwork_tasks(
    cached_ids: &HashSet<&String>,
    albums_vm: AlbumsService,
    album_ids_urls: Vec<(String, String)>,
) -> Vec<Task<Message>> {
    album_ids_urls
        .into_iter()
        .filter(|(id, _)| !cached_ids.contains(id))
        .map(|(id, url)| {
            let vm = albums_vm.clone();
            Task::perform(
                async move {
                    let handle = vm
                        .fetch_artwork_by_url_with_retry(&url)
                        .await
                        .ok()
                        .map(image::Handle::from_bytes);
                    (id, handle)
                },
                |(id, handle)| Message::Artwork(ArtworkMessage::Loaded(id, handle)),
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
    /// Universal block/transition checks every Play* handler needs:
    /// returns `Some(Task::none())` with a warning toast when in playlist
    /// edit mode (play would replace the queue being edited), or transitions
    /// radio playback back to queue mode when active. Returns `None` to let
    /// the caller proceed.
    ///
    /// Play actions that **replace queue contents** (album/artist/genre/
    /// playlist/song/batch/roulette) should additionally call
    /// [`Self::enter_new_playback_context`]. Play actions that only advance
    /// the playback pointer within the existing queue (`PlaySong` inside the
    /// queue view) must NOT — doing so clears the loaded-playlist header.
    pub(crate) fn guard_play_action(&mut self) -> Option<Task<Message>> {
        if self.playlist_edit.is_some() {
            self.toast_warn("Cannot play — would replace the playlist being edited");
            return Some(Task::none());
        }
        // NOTE from Claude: The plan says "Stop radio if active — transition back
        // to queue mode". The play action that follows will stop the engine anyway.
        // Blocking here with a toast prevents the user from ever resuming queue
        // playback while a radio stream is active — which defeats the purpose.
        if self.active_playback.is_radio() {
            self.active_playback = crate::state::ActivePlayback::Queue;
            // Engine stop is handled by the play action that follows
        }
        None
    }

    /// Reset state tied to the *previous* playback context.
    ///
    /// Call from play handlers that mutate the queue (replace, append-and-play,
    /// roulette pick, batch play). Skip from `QueueAction::PlaySong` — that
    /// path only moves the current-track pointer and must preserve the loaded
    /// playlist header.
    pub(crate) fn enter_new_playback_context(&mut self) {
        // Cancel any in-progress progressive queue loading target so the header
        // doesn't show a stale "X of Y" count from a superseded play action.
        self.library.queue_loading_target = None;
        self.clear_active_playlist();
    }

    /// Clear the active playlist context and persist the change.
    ///
    /// Single point of control — replaces the repeated two-line pattern
    /// `self.active_playlist_info = None; self.persist_active_playlist_info();`
    /// that was duplicated across 12+ call sites.
    pub(crate) fn clear_active_playlist(&mut self) {
        self.active_playlist_info = None;
        self.persist_active_playlist_info();
    }

    /// Persist the current `active_playlist_info` state to redb.
    ///
    /// Call after every mutation of `self.active_playlist_info` so the
    /// playlist context bar survives application restarts.
    pub(crate) fn persist_active_playlist_info(&self) {
        let (id, name, comment) = match &self.active_playlist_info {
            Some(ctx) => (Some(ctx.id.clone()), ctx.name.clone(), ctx.comment.clone()),
            None => (None, String::new(), String::new()),
        };
        self.shell_spawn("persist_active_playlist", move |shell| async move {
            shell
                .settings()
                .set_active_playlist(id, name, comment)
                .await
        });
    }

    /// Persist a column visibility toggle for any view whose column enum implements `ColumnPersist`.
    /// Replaces the 7 per-view `persist_*_column_visibility` helpers.
    pub(crate) fn persist_column_visibility<C: nokkvi_data::services::settings::ColumnPersist>(
        &self,
        col: C,
        value: bool,
    ) -> Task<Message> {
        self.shell_spawn("persist_column_visibility", move |shell| async move {
            shell.settings().set_column_visibility(col, value).await
        });
        Task::none()
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
                        if e.downcast_ref::<NokkviError>()
                            .is_some_and(|err| matches!(err, NokkviError::Unauthorized))
                        {
                            return Message::SessionExpired;
                        }
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
                        if e.downcast_ref::<NokkviError>()
                            .is_some_and(|err| matches!(err, NokkviError::Unauthorized))
                        {
                            return Message::SessionExpired;
                        }
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
                        if e.downcast_ref::<NokkviError>()
                            .is_some_and(|err| matches!(err, NokkviError::Unauthorized))
                        {
                            return Message::SessionExpired;
                        }
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
                if e.downcast_ref::<NokkviError>()
                    .is_some_and(|err| matches!(err, NokkviError::Unauthorized))
                {
                    return Message::SessionExpired;
                }
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
                if e.downcast_ref::<NokkviError>()
                    .is_some_and(|err| matches!(err, NokkviError::Unauthorized))
                {
                    return Message::SessionExpired;
                }
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

    /// Execute a radio API mutation, showing a success toast and reloading
    /// the station list on success. DRYs the create/edit/delete radio handlers.
    pub(crate) fn radio_mutation_task<F, Fut>(
        &self,
        api_fn: F,
        success_label: impl Into<String> + Send + 'static,
        error_ctx: &'static str,
    ) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::services::api::radios::RadiosApiService) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let success_label = success_label.into();
        self.shell_task(
            move |shell| async move {
                let service = shell.radios_api().await?;
                api_fn(service).await
            },
            move |result: Result<(), anyhow::Error>| match result {
                Ok(()) => Message::Toast(crate::app_message::ToastMessage::PushThen(
                    nokkvi_data::types::toast::Toast::new(
                        success_label,
                        nokkvi_data::types::toast::ToastLevel::Success,
                    ),
                    Box::new(Message::LoadRadioStations),
                )),
                Err(e) => {
                    if e.downcast_ref::<NokkviError>()
                        .is_some_and(|err| matches!(err, NokkviError::Unauthorized))
                    {
                        return Message::SessionExpired;
                    }
                    tracing::error!(" Failed to {}: {e}", error_ctx);
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to {error_ctx}: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
        )
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
            views::CommonViewAction::RefreshViewData => {
                // Return the reload message to bust the cache and refetch from source
                Some(Task::done(reload_msg))
            }
            views::CommonViewAction::CenterOnPlaying => Some(Task::done(Message::Hotkey(
                crate::app_message::HotkeyMessage::CenterOnPlaying,
            ))),
            views::CommonViewAction::NavigateAndFilter(view, filter) => {
                if self.browsing_panel.is_some() && self.current_view == crate::View::Queue {
                    let browse_view = match view {
                        crate::View::Albums => Some(crate::views::BrowsingView::Albums),
                        crate::View::Songs => Some(crate::views::BrowsingView::Songs),
                        crate::View::Artists => Some(crate::views::BrowsingView::Artists),
                        crate::View::Genres => Some(crate::views::BrowsingView::Genres),
                        crate::View::Queue
                        | crate::View::Playlists
                        | crate::View::Radios
                        | crate::View::Settings => None,
                    };
                    if browse_view.is_some() {
                        return Some(Task::done(Message::BrowserPaneNavigateAndFilter(
                            view, filter,
                        )));
                    }
                }
                Some(Task::done(Message::NavigateAndFilter(view, filter)))
            }
            views::CommonViewAction::NavigateAndExpandAlbum(album_id) => {
                if self.browsing_panel.is_some() && self.current_view == crate::View::Queue {
                    return Some(Task::done(Message::BrowserPaneNavigateAndExpandAlbum {
                        album_id,
                    }));
                }
                Some(Task::done(Message::NavigateAndExpandAlbum { album_id }))
            }
            views::CommonViewAction::NavigateAndExpandArtist(artist_id) => {
                if self.browsing_panel.is_some() && self.current_view == crate::View::Queue {
                    return Some(Task::done(Message::BrowserPaneNavigateAndExpandArtist {
                        artist_id,
                    }));
                }
                Some(Task::done(Message::NavigateAndExpandArtist { artist_id }))
            }
            views::CommonViewAction::NavigateAndExpandGenre(genre_id) => {
                if self.browsing_panel.is_some() && self.current_view == crate::View::Queue {
                    return Some(Task::done(Message::BrowserPaneNavigateAndExpandGenre {
                        genre_id,
                    }));
                }
                Some(Task::done(Message::NavigateAndExpandGenre { genre_id }))
            }
            views::CommonViewAction::None | views::CommonViewAction::ViewSpecific => None,
        }
    }

    /// Star or unstar an item via the Subsonic API.
    /// Optimistic local state updates should be done inline at the call site before calling this.
    /// On failure, emits a revert message to restore the original starred state.
    pub(crate) fn star_item_task(&self, id: String, kind: ItemKind, star: bool) -> Task<Message> {
        let action = if star { "star" } else { "unstar" };
        let revert_id = id.clone();
        debug!(
            " {} {} {}",
            if star { "Starring" } else { "Unstarring" },
            kind,
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
                        kind.api_str(),
                    )
                    .await
                } else {
                    nokkvi_data::services::api::star::unstar_item(
                        &client.http_client(),
                        &server_url,
                        &subsonic_credential,
                        &id,
                        kind.api_str(),
                    )
                    .await
                }
            },
            move |result| {
                if let Err(e) = result {
                    if e.downcast_ref::<NokkviError>()
                        .is_some_and(|err| matches!(err, NokkviError::Unauthorized))
                    {
                        return Message::SessionExpired;
                    }
                    error!(" Failed to {} {}: {}", action, kind, e);
                    // Revert optimistic update by emitting the original starred state
                    return Self::starred_revert_message(revert_id, kind, !star);
                }
                Message::NoOp
            },
        )
    }

    /// Build the appropriate starred-status-updated message for a given item kind.
    /// Used to revert optimistic star updates on API failure.
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

    /// Toggle a star on any item, applying an optimistic UI update that reverts on API failure.
    pub(crate) fn toggle_star_with_revert_task(
        &self,
        id: String,
        kind: ItemKind,
        star: bool,
    ) -> Task<Message> {
        let optimistic_msg = Self::starred_revert_message(id.clone(), kind, star);
        Task::batch(vec![
            Task::done(optimistic_msg),
            self.star_item_task(id, kind, star),
        ])
    }

    /// Build the appropriate rating-updated message for a given item kind.
    /// Used to revert optimistic rating updates on API failure.
    pub(crate) fn rating_revert_message(id: String, kind: ItemKind, rating: u32) -> Message {
        use HotkeyMessage::{AlbumRatingUpdated, ArtistRatingUpdated, SongRatingUpdated};
        Message::Hotkey(match kind {
            ItemKind::Album => AlbumRatingUpdated(id, rating),
            ItemKind::Artist => ArtistRatingUpdated(id, rating),
            ItemKind::Song | ItemKind::Playlist => SongRatingUpdated(id, rating),
        })
    }

    /// Set an absolute rating on an item via the Subsonic API.
    /// Applies optimistic UI update immediately, reverts on failure.
    pub(crate) fn set_item_rating_task(
        &self,
        id: String,
        kind: ItemKind,
        new_rating: usize,
        current_rating: u32,
    ) -> Task<Message> {
        let new_rating_u32 = new_rating as u32;
        debug!(
            "⭐ Setting rating for {} {}: {} -> {}",
            kind, id, current_rating, new_rating
        );

        // Optimistic update
        let optimistic_msg = Self::rating_revert_message(id.clone(), kind, new_rating_u32);

        let revert_id = id.clone();
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
                    if e.downcast_ref::<NokkviError>()
                        .is_some_and(|err| matches!(err, NokkviError::Unauthorized))
                    {
                        return Message::SessionExpired;
                    }
                    error!(" Failed to set rating: {}", e);
                    Self::rating_revert_message(revert_id, kind, current_rating)
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

    /// Resolve a `BatchPayload` into song IDs, fetch playlists, and open the dialog.
    pub(crate) fn handle_add_batch_to_playlist(
        &self,
        batch: nokkvi_data::types::batch::BatchPayload,
    ) -> Task<Message> {
        let len = batch.items.len();
        debug!(" Fetching playlists for batch of {} items", len);
        self.resolve_and_add_to_playlist(
            move |shell| async move {
                let songs = shell.resolve_batch(batch).await?;
                Ok(songs.into_iter().map(|s| s.id).collect())
            },
            "resolve batch for playlist",
        )
    }

    /// Enqueue a batch, inserting at a drag-drop position when one is pending.
    ///
    /// Takes `pending_queue_insert_position` via `take()` — the position is consumed
    /// even when the insert path is not taken, so callers must not pre-take it.
    pub(crate) fn add_or_insert_batch_to_queue_task(
        &mut self,
        payload: nokkvi_data::types::batch::BatchPayload,
    ) -> Task<Message> {
        let len = payload.items.len();
        if let Some(pos) = self.pending_queue_insert_position.take() {
            return self.shell_fire_and_forget_task(
                move |shell| async move { shell.insert_batch_at_position(payload, pos).await },
                format!("Inserted {len} items at position {}", pos + 1),
                "insert batch to queue",
            );
        }
        self.shell_fire_and_forget_task(
            move |shell| async move { shell.add_batch_to_queue(payload).await },
            format!("Added {len} items to queue"),
            "add batch to queue",
        )
    }

    /// Fire a play-next-batch task, warning if shuffle is active.
    pub(crate) fn play_next_batch_task(
        &mut self,
        payload: nokkvi_data::types::batch::BatchPayload,
    ) -> Task<Message> {
        if self.modes.random {
            self.toast_warn("Shuffle is on — next tracks will be random, not these");
        }
        self.shell_fire_and_forget_task(
            move |shell| async move { shell.play_next_batch(payload).await },
            "Added batch to play next".to_string(),
            "play next batch",
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
                if e.downcast_ref::<NokkviError>()
                    .is_some_and(|err| matches!(err, NokkviError::Unauthorized))
                {
                    return Message::SessionExpired;
                }
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

    /// Fetch a representative song path for an album and open its containing folder.
    ///
    /// Albums don't carry file paths directly — this fetches the first song
    /// to obtain a path, then dispatches `Message::ShowInFolder`. Used by
    /// albums, artists, and info modal handlers.
    pub(crate) fn show_album_in_folder_task(&self, album_id: String) -> Task<Message> {
        self.shell_task(
            move |shell| async move {
                let songs = shell.albums().load_album_songs(&album_id).await?;
                songs
                    .first()
                    .map(|s| s.path.clone())
                    .ok_or_else(|| anyhow::anyhow!("Album has no songs"))
            },
            |result: Result<String, anyhow::Error>| match result {
                Ok(path) => Message::ShowInFolder(path),
                Err(e) => {
                    tracing::error!("Failed to load album path: {e}");
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to open folder: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
        )
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
        let optimistic_msg =
            Self::starred_revert_message(song_id.clone(), ItemKind::Song, new_starred);

        // API call
        let api_task = self.star_item_task(song_id, ItemKind::Song, new_starred);

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

    /// Fetch a song's file path from the API and dispatch `ShowInFolder`.
    /// Shared by queue ShowInFolder and strip context menu ShowInFolder.
    pub(crate) fn show_song_in_folder_task(&self, song_id: String) -> Task<Message> {
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

    /// Open Find Similar tab for the currently playing track.
    pub(crate) fn handle_find_similar_for_playing_track(&mut self) -> Task<Message> {
        let Some(song_id) = self.scrobble.current_song_id.clone() else {
            self.toast_warn("No track is currently playing");
            return Task::none();
        };
        let title = self.playback.title.clone();
        Task::done(Message::FindSimilar {
            id: song_id,
            label: format!("Similar to: {title}"),
        })
    }

    /// Open Top Songs tab for the currently playing track's artist.
    pub(crate) fn handle_find_top_songs_for_playing_track(&mut self) -> Task<Message> {
        let artist = self.playback.artist.clone();
        if artist.is_empty() {
            self.toast_warn("No artist metadata for currently playing track");
            return Task::none();
        }
        Task::done(Message::FindTopSongs {
            artist_name: artist.clone(),
            label: format!("Top Songs: {artist}"),
        })
    }

    /// Open the currently playing track's folder in the file manager.
    pub(crate) fn handle_show_in_folder_for_playing_track(&mut self) -> Task<Message> {
        let Some(song_id) = self.scrobble.current_song_id.clone() else {
            self.toast_warn("No track is currently playing");
            return Task::none();
        };
        self.show_song_in_folder_task(song_id)
    }

    // -----------------------------------------------------------------
    // Strip navigation helpers (shared by StripClicked + StripContextAction)
    // -----------------------------------------------------------------

    /// Navigate to a view from the metadata strip.
    /// When `center_on_playing` is true (context menu), also scroll the
    /// target view to the currently playing track.
    pub(crate) fn strip_navigate(&mut self, view: View, center_on_playing: bool) -> Task<Message> {
        let switch = self.handle_switch_view(view);
        if center_on_playing {
            let center = self.handle_center_on_playing();
            Task::batch([switch, center])
        } else {
            switch
        }
    }

    /// Copy "Artist — Title" to clipboard and show a toast.
    pub(crate) fn strip_copy_track_info(&mut self) -> Task<Message> {
        let info = format!("{} — {}", self.playback.artist, self.playback.title);
        self.toast_info("Copied to clipboard");
        iced::clipboard::write(info).map(|_| Message::NoOp)
    }
}
