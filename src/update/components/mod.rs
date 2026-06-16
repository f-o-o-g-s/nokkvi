//! Shared component handler utilities.
//!
//! This module contains only shared helpers used by the per-view handler files
//! (`albums.rs`, `artists.rs`, `songs.rs`, `genres.rs`, `playlists.rs`, `queue.rs`, `player_bar.rs`).
//! The canonical artwork-prefetch helpers live in the [`artwork_prefetch`]
//! submodule and are re-exported here, so callers keep using `components::<fn>`.
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
//! - Artwork prefetching → the `artwork_prefetch` submodule (re-exported here)
use iced::Task;
use nokkvi_data::types::{ItemKind, error::NokkviError};
use tracing::{debug, error, info};

use crate::{
    Nokkvi, View,
    app_message::{FindMessage, HotkeyMessage, Message, NavigationMessage},
    views,
    widgets::{SlotListPageState, view_header::SortMode},
};

mod artwork_prefetch;
// Canonical artwork-prefetch helpers live in the `artwork_prefetch` submodule
// (the one path-reached unit here); re-exported so call sites keep using
// `components::<fn>` unchanged.
// `should_refetch` is exercised directly only by the artwork dedup tests; its
// production callers live inside `artwork_prefetch`, so re-export it for tests.
#[cfg(test)]
pub(super) use artwork_prefetch::should_refetch;
pub(super) use artwork_prefetch::{
    expansion_album_artwork_tasks, expansion_child_album_ids, passive_artwork_version,
    prefetch_album_artwork_tasks, prefetch_quad_album_artwork_tasks, prefetch_song_artwork_tasks,
};

/// Map an `anyhow::Error` chain to [`Message::SessionExpired`] when its
/// underlying cause is [`NokkviError::Unauthorized`] — the canonical
/// "JWT expired, drop to login" signal. Returns `None` for any other error,
/// so callers fall through to their normal error-toast path.
///
/// Pair partner of `subsonic_post_ok` / `ApiClient::{get,post_json,put_json,
/// delete}`, which route HTTP 401 → `NokkviError::Unauthorized` at the API
/// boundary. This helper consolidates the inverse: catching the typed error
/// inside `Result<_, anyhow::Error>` results bubbled back to UI handlers.
pub(crate) fn session_expired_message(e: &anyhow::Error) -> Option<Message> {
    e.downcast_ref::<NokkviError>()
        .is_some_and(|err| matches!(err, NokkviError::Unauthorized))
        .then_some(Message::SessionExpired)
}

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
        if self.playlist_editor.is_some() {
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
        // Drop any stale strip expansion so it never carries into the next
        // playlist (or shows over an empty context).
        self.queue_page.playlist_strip_expanded = false;
        // The strip quad identity belongs to the context — drop it with the
        // context so the next playlist's `handle_queue_loaded` re-freezes it
        // from its own queue head.
        self.strip_quad_album_ids.clear();
        self.persist_active_playlist_info();
    }

    /// Persist the current `active_playlist_info` state to redb.
    ///
    /// Call after every mutation of `self.active_playlist_info` so the
    /// playlist context bar survives application restarts.
    pub(crate) fn persist_active_playlist_info(&self) {
        let (id, name, comment, duration, updated, public, song_count) =
            match &self.active_playlist_info {
                Some(ctx) => (
                    Some(ctx.id.clone()),
                    ctx.name.clone(),
                    ctx.comment.clone(),
                    ctx.duration_secs,
                    ctx.updated.clone(),
                    ctx.public,
                    ctx.song_count,
                ),
                None => (
                    None,
                    String::new(),
                    String::new(),
                    0.0,
                    String::new(),
                    false,
                    0,
                ),
            };
        self.shell_spawn("persist_active_playlist", move |shell| async move {
            shell
                .settings()
                .set_active_playlist(id, name, comment, duration, updated, public, song_count)
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
                    Ok(()) => Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
                    Err(e) => {
                        if let Some(msg) = session_expired_message(&e) {
                            return msg;
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
                        if let Some(msg) = session_expired_message(&e) {
                            return msg;
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
                        if let Some(msg) = session_expired_message(&e) {
                            return msg;
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
    ///     Message::Navigation(NavigationMessage::SwitchView(View::Queue)),
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
                if let Some(msg) = session_expired_message(&e) {
                    return msg;
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

    /// Load expansion children on the shell (album tracks, artist albums,
    /// genre albums, playlist tracks), mapping success through
    /// `into_loaded_msg` and failure through the shared session-expired
    /// check and error-toast tail. The four `Expand*` handler arms route
    /// through here so the Ok→`*Loaded` / Err→(`SessionExpired` | `error!`
    /// and toast) shape lives at one site.
    ///
    /// Per-view artwork/collage side tasks stay at the call sites (they differ
    /// per view and need `&mut self` before the closures capture).
    pub(crate) fn expand_load_children_task<F, Fut, C>(
        &self,
        load_fn: F,
        into_loaded_msg: impl FnOnce(C) -> Message + Send + 'static,
        error_ctx: &'static str,
    ) -> Task<Message>
    where
        F: FnOnce(nokkvi_data::backend::app_service::AppService) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<C, anyhow::Error>> + Send,
        C: Send + 'static,
    {
        self.shell_task(load_fn, move |result| match result {
            Ok(children) => into_loaded_msg(children),
            Err(e) => {
                if let Some(msg) = session_expired_message(&e) {
                    return msg;
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

    /// Append the expansion-children mini-artwork prefetch fan-out to a
    /// page-update task. Shared tail of the Artists and Genres handlers:
    /// returns `cmd_task` untouched when no children were newly loaded (or
    /// pre-login), otherwise batches the version-gated 80px fetches behind it.
    pub(crate) fn append_expansion_album_prefetch(
        &self,
        cmd_task: Task<Message>,
        album_ids: Vec<(String, Option<String>, String)>,
    ) -> Task<Message> {
        if album_ids.is_empty() {
            return cmd_task;
        }
        let Some(shell) = &self.app_service else {
            return cmd_task;
        };
        let cached: std::collections::HashSet<&String> =
            self.artwork.album_art.iter().map(|(k, _)| k).collect();
        let prefetch = expansion_album_artwork_tasks(
            &cached,
            &self.artwork.album_art_versions,
            &self.artwork.failed_art,
            &self.artwork.album_art_pending,
            shell.albums().clone(),
            album_ids,
        );
        if prefetch.is_empty() {
            cmd_task
        } else {
            let mut tasks = vec![cmd_task];
            tasks.extend(prefetch);
            Task::batch(tasks)
        }
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
                if let Some(msg) = session_expired_message(&e) {
                    return msg;
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
                    if let Some(msg) = session_expired_message(&e) {
                        return msg;
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
                        | crate::View::Settings
                        | crate::View::PlaylistEditor => None,
                    };
                    if browse_view.is_some() {
                        return Some(Task::done(Message::Navigation(
                            NavigationMessage::NavigateAndFilter {
                                view,
                                filter,
                                for_browsing_pane: true,
                            },
                        )));
                    }
                }
                Some(Task::done(Message::Navigation(
                    NavigationMessage::NavigateAndFilter {
                        view,
                        filter,
                        for_browsing_pane: false,
                    },
                )))
            }
            views::CommonViewAction::NavigateAndExpandAlbum(album_id) => {
                if self.browsing_panel.is_some() && self.current_view == crate::View::Queue {
                    return Some(Task::done(Message::Navigation(NavigationMessage::Expand(
                        crate::state::PendingExpand::Album {
                            album_id,
                            for_browsing_pane: true,
                        },
                    ))));
                }
                Some(Task::done(Message::Navigation(NavigationMessage::Expand(
                    crate::state::PendingExpand::Album {
                        album_id,
                        for_browsing_pane: false,
                    },
                ))))
            }
            views::CommonViewAction::NavigateAndExpandArtist(artist_id) => {
                if self.browsing_panel.is_some() && self.current_view == crate::View::Queue {
                    return Some(Task::done(Message::Navigation(NavigationMessage::Expand(
                        crate::state::PendingExpand::Artist {
                            artist_id,
                            for_browsing_pane: true,
                        },
                    ))));
                }
                Some(Task::done(Message::Navigation(NavigationMessage::Expand(
                    crate::state::PendingExpand::Artist {
                        artist_id,
                        for_browsing_pane: false,
                    },
                ))))
            }
            views::CommonViewAction::NavigateAndExpandGenre(genre_id) => {
                if self.browsing_panel.is_some() && self.current_view == crate::View::Queue {
                    return Some(Task::done(Message::Navigation(NavigationMessage::Expand(
                        crate::state::PendingExpand::Genre {
                            genre_id,
                            for_browsing_pane: true,
                        },
                    ))));
                }
                Some(Task::done(Message::Navigation(NavigationMessage::Expand(
                    crate::state::PendingExpand::Genre {
                        genre_id,
                        for_browsing_pane: false,
                    },
                ))))
            }
            views::CommonViewAction::None | views::CommonViewAction::ViewSpecific => None,
        }
    }

    /// Look up an item by id and return its rating, defaulting to 0 on miss.
    ///
    /// Used by every `SetRating` handler to read the item's current rating
    /// before dispatching the optimistic update + API call — the optimistic
    /// revert needs the prior value if the API errors. Defaulting to 0 on
    /// miss matches the original inline behavior (`.unwrap_or(0)`).
    ///
    /// Generic over `T` and the rating accessor closure so the same helper
    /// serves AlbumUIViewData, ArtistUIViewData, SongUIViewData,
    /// QueueSongUIViewData, etc. — the per-entity rating field name
    /// (`a.rating`, `s.rating`) stays at the call site.
    pub(crate) fn find_current_rating<T>(
        items: &[T],
        id: &str,
        get_id: impl Fn(&T) -> &str,
        get_rating: impl Fn(&T) -> Option<u32>,
    ) -> u32 {
        items
            .iter()
            .find(|item| get_id(item) == id)
            .and_then(get_rating)
            .unwrap_or(0)
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
                let (server_url, subsonic_credential) = auth_vm.server_config().await;
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
                    if let Some(msg) = session_expired_message(&e) {
                        return msg;
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
                let (server_url, subsonic_credential) = auth_vm.server_config().await;

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
                    if let Some(msg) = session_expired_message(&e) {
                        return msg;
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
                let library_ids = shell.active_library_ids_vec();
                let (playlists, _) = service
                    .load_playlists_with_libraries("name", "ASC", None, &library_ids)
                    .await?;
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

    /// Redirect a play action to a queue-add task when the browsing panel is open.
    ///
    /// All five library views (Albums, Artists, Genres, Playlists, Songs) share
    /// the same play-redirect shape inside split-view: when `browsing_panel` is
    /// present, instead of replacing the queue, they enqueue the entity — at a
    /// drag-drop target position if one is pending, else appended to the end.
    /// Per-entity differences (id parsing, name lookup, async backend call)
    /// stay in the caller's `add_task` / `insert_task` closures, so this helper
    /// owns only the shared shape:
    /// - Returns `None` when `browsing_panel` is closed (caller proceeds with
    ///   normal play flow).
    /// - Returns `Some(insert_task(pos))` when a drag-drop position is pending
    ///   (consumes it via `take()`).
    /// - Otherwise returns `Some(add_task())`.
    ///
    /// **Contract**: call this AFTER `guard_play_action()` has returned `None`
    /// — the helper does not re-guard. Pairing with `enter_new_playback_context`
    /// is per-site (Songs skips it inside the browsing-panel branch; the four
    /// entity sites call it before this helper).
    pub(crate) fn redirect_play_to_queue_in_browsing_panel<A, I>(
        &mut self,
        add_task: A,
        insert_task: I,
    ) -> Option<Task<Message>>
    where
        A: FnOnce(&mut Self) -> Task<Message>,
        I: FnOnce(&mut Self, usize) -> Task<Message>,
    {
        self.browsing_panel.as_ref()?;
        if let Some(pos) = self.cross_pane_drag.pending_queue_insert_position.take() {
            return Some(insert_task(self, pos));
        }
        Some(add_task(self))
    }

    /// Enqueue a batch, inserting at a drag-drop position when one is pending.
    ///
    /// Takes `cross_pane_drag.pending_queue_insert_position` via `take()` — the
    /// position is consumed even when the insert path is not taken, so callers
    /// must not pre-take it.
    pub(crate) fn add_or_insert_batch_to_queue_task(
        &mut self,
        payload: nokkvi_data::types::batch::BatchPayload,
    ) -> Task<Message> {
        let len = payload.items.len();
        if let Some(pos) = self.cross_pane_drag.pending_queue_insert_position.take() {
            // While a playlist edit session is active the drop target is the
            // editor's LEFT pane, so resolve the dragged item(s) into editor
            // view-data rows and splice them into the editor buffer at the drop
            // slot — the live queue / engine / redb are never touched (plan
            // §5.6). Resolving is async because building rows needs the
            // server_url/credential for artwork URLs, mirroring the queue
            // insert path's data flow.
            if self.playlist_editor.is_some() {
                return self.shell_task(
                    move |shell| async move { shell.resolve_batch_for_editor(payload).await },
                    move |result| match result {
                        Ok(rows) => {
                            Message::Editor(crate::app_message::EditorMessage::SongsInserted {
                                rows,
                                at: pos,
                            })
                        }
                        Err(e) => {
                            error!(" Failed to resolve dragged batch for editor: {}", e);
                            Message::NoOp
                        }
                    },
                );
            }
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

    /// Replace the queue with a batch and navigate to the Queue view.
    ///
    /// Sibling of [`Self::add_or_insert_batch_to_queue_task`] /
    /// [`Self::play_next_batch_task`] for the third batch-action shape:
    /// queue replacement + navigation, used by Albums and Songs PlayBatch arms.
    /// The helper always clears `active_playlist_info` (since the queue is
    /// being replaced, the previously-loaded-playlist header is no longer
    /// accurate) and uses `shell_task` + `Navigation::SwitchView(Queue)` on
    /// success — matching the existing Albums/Songs UX.
    ///
    /// Callers should clear their per-view `selected_indices` BEFORE invoking
    /// this helper (selection state is per-view and not accessible from here).
    /// Similar's PlayBatch deliberately uses `shell_fire_and_forget_task` +
    /// toast (no navigation) because it lives in the browsing panel where
    /// the user is already viewing the queue — Similar does not call this
    /// helper.
    pub(crate) fn play_batch_task(
        &mut self,
        payload: nokkvi_data::types::batch::BatchPayload,
    ) -> Task<Message> {
        let len = payload.items.len();
        debug!(" Playing batch of {} items", len);
        self.clear_active_playlist();
        self.shell_task(
            move |shell| async move { shell.play_batch(payload).await },
            move |result| match result {
                Ok(()) => Message::Navigation(crate::app_message::NavigationMessage::SwitchView(
                    crate::View::Queue,
                )),
                Err(e) => {
                    if let Some(msg) = session_expired_message(&e) {
                        return msg;
                    }
                    error!(" Failed to play batch: {}", e);
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            format!("Failed to play batch: {e}"),
                            nokkvi_data::types::toast::ToastLevel::Error,
                        ),
                    ))
                }
            },
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
                if let Some(msg) = session_expired_message(&e) {
                    return msg;
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

    /// Handle a `TaskStatusChanged` notification from the centralized task
    /// manager — surface failures as toasts, log lifecycle transitions for
    /// forensics. `Running` traces, `Completed` / `Cancelled` debug, `Failed`
    /// toasts the user. Returns `Task::none()` (no follow-up Task).
    ///
    /// Extracted from the inline arm in `update/mod.rs` so the central
    /// dispatcher matches every other Message arm's one-line delegation
    /// shape.
    pub(crate) fn handle_task_status_changed(
        &mut self,
        handle: nokkvi_data::services::task_manager::TaskHandle,
        status: nokkvi_data::services::task_manager::TaskStatus,
    ) -> Task<Message> {
        use nokkvi_data::services::task_manager::TaskStatus;
        match status {
            TaskStatus::Running => {
                // Optional: update active progress list or show a toast
                tracing::trace!(" [TASK] {} is running", handle.name);
            }
            TaskStatus::Completed => {
                tracing::debug!(" [TASK] {} completed", handle.name);
            }
            TaskStatus::Failed(e) => {
                self.toast_error(format!("Task failed: {} - {}", handle.name, e));
            }
            TaskStatus::Cancelled => {
                tracing::debug!(" [TASK] {} cancelled", handle.name);
            }
        }
        Task::none()
    }

    /// Open the containing folder of a song file in the user's file manager.
    ///
    /// `relative_path` is the song's path as stored by Navidrome (relative to the
    /// music library root). The method prepends `self.settings.local_music_path`, resolves
    /// the parent directory, and opens it with `xdg-open`.
    pub(crate) fn handle_show_in_folder(&mut self, relative_path: String) -> Task<Message> {
        if self.settings.local_music_path.is_empty() {
            self.toast_warn(
                "Set a Local Music Path in Settings → Application to open files in your file manager.",
            );
            return Task::none();
        }

        let prefix = self.settings.local_music_path.trim_end_matches('/');
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
        Task::done(Message::Find(FindMessage::Similar {
            id: song_id,
            label: format!("Similar to: {title}"),
        }))
    }

    /// Open Top Songs tab for the currently playing track's artist.
    pub(crate) fn handle_find_top_songs_for_playing_track(&mut self) -> Task<Message> {
        let artist = self.playback.artist.clone();
        if artist.is_empty() {
            self.toast_warn("No artist metadata for currently playing track");
            return Task::none();
        }
        Task::done(Message::Find(FindMessage::TopSongs {
            artist_name: artist.clone(),
            label: format!("Top Songs: {artist}"),
        }))
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

    // -----------------------------------------------------------------
    // Session teardown (logout + session-expired share this)
    // -----------------------------------------------------------------

    /// Tear down all session-bound state and return the engine-stop Task.
    ///
    /// Both `handle_settings_logout` (user-initiated) and
    /// `handle_session_expired` (401 from API) end the active Navidrome
    /// session and must leave `Nokkvi` in an identical post-teardown shape
    /// before re-arriving at the Login screen. This helper is the single
    /// source of truth for that shape, so a future field added to `Nokkvi`
    /// gets reset uniformly from one site instead of drifting between the
    /// two callers (the original split forgot `open_menu` + `library` in
    /// the logout path — see #2.27).
    ///
    /// The async Task returned must be awaited by the caller (returned
    /// from the handler) so PipeWire streams, the decode loop, and the
    /// render thread are torn down cleanly — preventing orphaned audio
    /// after logout. `StateStorage` is cached on `self.cached_storage`
    /// (the redb exclusive lock would block a fresh open on re-login —
    /// see gotchas.md "Database lock on re-login").
    ///
    /// Caller-specific concerns (toast, log prefix) stay at the call
    /// site; this helper logs a single `debug!` line summarizing the
    /// reset for forensic traces.
    ///
    /// Reset fields fall into three buckets:
    /// - **Core session identity**: app_service, stored_session,
    ///   should_auto_login, screen.
    /// - **Server-specific data pointing at gone IDs**: library, artwork,
    ///   similar_songs(+generation), active_playlist_info, playlist_editor,
    ///   server_version, last_queue_current_index,
    ///   pending_expand (whole `PendingExpandState`), roulette.
    /// - **Transient UI work tied to the prior session**: open_menu,
    ///   browsing_panel, cross_pane_drag (whole `CrossPaneDragUi`),
    ///   start_view_applied, suppress_next_auto_center.
    ///
    /// Side-effect calls (not field resets):
    /// - `services::navidrome_sse::clear()` drops the static SSE
    ///   connection registration so the event loop can't keep retrying
    ///   with stale credentials against the prior server (would 401
    ///   forever until next `register()`).
    ///
    /// Fields explicitly NOT reset (retained across login transitions):
    /// - retained: cached_storage — explicit DB-lock workaround.
    /// - retained: login_page — credentials kept so user can re-enter.
    /// - retained: current_view, pre_settings_view — UI nav memory.
    /// - retained: modes, settings, hotkey_config — user preferences.
    /// - retained: sfx_engine, sfx, engine, window, player_bar_layout,
    ///   visualizer(+config), boat — local UI/audio infrastructure
    ///   independent of the server.
    /// - retained: toast, text_input_dialog, info_modal, about_modal,
    ///   eq_modal, default_playlist_picker — modal/overlay shells
    ///   (toast queue intentionally survives so the session-expired
    ///   message is visible after this returns).
    /// - retained: mpris_connection, tray_connection, tray_window_hidden,
    ///   main_window_id — system integrations bound to the app process,
    ///   not the session.
    /// - retained: playback, active_playback, scrobble — track-display
    ///   fields. Engine-stop is async; resetting these here could race.
    ///   They are overwritten on next session's first queue load and the
    ///   Login screen doesn't render the player bar.
    /// - retained: last_mpris_position_us — overwritten on next playback.
    pub(crate) fn reset_session_state(&mut self) -> Task<Message> {
        // Phase 1: cache the storage handle for re-login, then build a single
        // async teardown Task that — in this strict order — (1) stops the audio
        // engine, (2) drains the TaskManager so every tracked persistence /
        // credential write either finishes its synchronous redb commit or is
        // aborted, and only THEN (3) clears the redb session. Sequencing
        // clear_session AFTER the awaited drain gives the happens-before edge a
        // straggler queue/credential writer would otherwise race past (N3/N14):
        // an in-flight save_session / save_jwt_token / queue-save task can no
        // longer re-materialize a stale or non-empty blob after the clear.
        // Mirrors AppService::request_shutdown's engine-first, drain-second
        // shape and reuses its 500 ms budget.
        let stop_task = if let Some(ref shell) = self.app_service {
            // Cache the storage Arc before `self.app_service = None` drops the
            // shell. This is the same DB handle the async body clears — caching
            // it early is harmless because the drain clears DB *contents*, not
            // the handle.
            self.cached_storage = Some(shell.storage().clone());

            let task_manager = shell.task_manager();
            let engine = shell.audio_engine();
            let storage = shell.storage().clone();
            Task::perform(
                async move {
                    // (1) Stop the engine first (kills PipeWire streams, the
                    // decode loop, and the render thread). Lock, stop, drop the
                    // guard BEFORE the drain — never hold the engine lock across
                    // shutdown_all.
                    {
                        let mut guard = engine.lock().await;
                        guard.stop().await;
                    }
                    tracing::debug!(" [SESSION-RESET] Audio engine stopped");

                    // (2) Drain tracked tasks within the established 500 ms
                    // budget; stragglers past budget are aborted.
                    let clean = task_manager
                        .shutdown_all(std::time::Duration::from_millis(500))
                        .await;
                    tracing::debug!(" [SESSION-RESET] task manager drained ({clean} clean)");

                    // (3) Now that no tracked credential/persistence writer can
                    // run after this point, clear the redb session last so it
                    // wins. Belt-and-braces for N14: even an untracked refresh
                    // that snuck a write in during the drain is overwritten.
                    if let Err(e) = nokkvi_data::credentials::clear_session(&storage) {
                        tracing::warn!(" [SESSION-RESET] Failed to clear session: {e}");
                    }
                },
                |()| Message::NoOp,
            )
        } else {
            Task::none()
        };

        // Phase 2: reset every session-bound field on Nokkvi.
        // Grouped by bucket (see doc-comment above).
        //
        // Core session identity
        self.app_service = None;
        self.stored_session = None;
        self.should_auto_login = false;
        self.screen = crate::Screen::Login;

        // Clear transient login state so re-login starts from a clean slate:
        // drop the prior password and any stale error / in-progress flag. The
        // server URL and username stay pre-filled for convenience.
        self.login_page.password.clear();
        self.login_page.error = None;
        self.login_page.login_in_progress = false;

        // Server-specific data pointing at gone IDs
        self.library = crate::state::LibraryData::default();
        // Artwork caches are session-bound: server-A's cover bytes must not be
        // served for server-B's album IDs if the IDs happen to overlap after
        // re-login. Default rebuilds the LRUs at their declared capacities.
        self.artwork = crate::state::ArtworkState::default();
        self.similar_songs = None;
        self.similar_songs_generation = 0;
        self.active_playlist_info = None;
        self.queue_page.playlist_strip_expanded = false;
        // A sort dropdown / hover lock set at logout or session-expiry would
        // otherwise survive into the next session: the pick_list / header
        // mouse_area unmount on the Login-screen swap, so their on_close /
        // on_exit can't fire to clear the flag.
        self.clear_all_toolbar_reveal_locks();
        self.playlist_editor = None;
        self.server_version = None;
        self.last_queue_current_index = None;
        self.pending_expand = crate::state::PendingExpandState::default();
        self.roulette = None;

        // Transient UI work tied to the prior session — including any
        // modals the user might have had open at logout. Without this
        // reset, logging in to a different server briefly shows the
        // prior server's About / Info / EQ / text-input / playlist-picker
        // overlay before the user dismisses it (the visible field bytes
        // are still wired to the previous session's data shapes).
        self.about_modal = crate::widgets::about_modal::AboutModalState::default();
        self.info_modal = crate::widgets::info_modal::InfoModalState::default();
        self.eq_modal = crate::widgets::eq_modal::EqModalState::default();
        self.text_input_dialog = crate::widgets::text_input_dialog::TextInputDialogState::default();
        self.default_playlist_picker = None;

        self.open_menu = None;
        self.browsing_panel = None;
        self.cross_pane_drag = crate::state::CrossPaneDragUi::default();
        self.start_view_applied = false;
        self.suppress_next_auto_center = false;
        self.pending_mode_commits = 0;

        // Drop the visualizer so its background FFT thread joins now,
        // not at next login's `self.visualizer = Some(new)` overwrite.
        // Without explicit cleanup the worker keeps spinning between
        // logout and re-login (or forever, if the user never re-logs).
        // Drop joins the thread within one `TICK_INTERVAL` (~16.67 ms).
        self.visualizer = None;

        // Drop the static SSE connection registration so the event loop
        // stops retrying against the prior server with stale credentials.
        // Without this, post-logout SSE attempts 401 indefinitely (or hang
        // against an unreachable host) until the next successful login
        // overwrites the slot via `register()`.
        crate::services::navidrome_sse::clear();

        // Drop the per-process MPRIS art cache (the file at
        // ~/.cache/nokkvi/mpris-art-<pid>.jpg plus the in-memory last-written
        // key). Without this, server-B's MPRIS metadata would emit a file://
        // URL whose contents are still server-A's bytes from before logout,
        // until the next track change forces a rewrite.
        let mpris_art_clear_task =
            Task::perform(crate::services::mpris_art_writer::clear(), |_| {
                Message::NoOp
            });

        tracing::debug!(" [SESSION-RESET] cleared session-bound state");

        // Re-focus the first login field now that we're back on the login
        // screen (the window already exists, so this lands on the next frame).
        let focus_task = Task::done(Message::Login(crate::views::LoginMessage::FocusFirstField));

        Task::batch([stop_task, mpris_art_clear_task, focus_task])
    }
}
