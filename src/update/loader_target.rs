//! Generic load-result handlers driven by per-view `LoaderTarget` specs.
//!
//! All five library views (Albums, Artists, Songs, Genres, Playlists) share
//! the same 9-step skeleton for handling initial loads and page appends.
//! `handle_loaded_with` and `handle_page_loaded_with` are the single bodies;
//! per-view differences live in the five zero-sized `*Target` marker structs.
//!
//! The three paged views (Albums, Artists, Songs) additionally share the
//! paged-fetch dispatch wrapper — `Nokkvi::load_paged<T>` owns the
//! page-size, defensive-gate, `PaginatedFetch` build, and `set_loading(true)`
//! sequence so the "always call `set_loading(true)` before dispatching"
//! invariant (CLAUDE.md gotcha — "rapid scroll triggers duplicate fetches"
//! otherwise) lives in one place. Per-view fetch closures supply only the
//! per-entity backend call and the UI-projection mapping.
//!
//! `dead_code` is suppressed at module level because no callers exist until
//! Lanes B and C migrate the existing handler bodies to delegate here.
#![allow(dead_code)]

use std::{collections::HashSet, future::Future};

use iced::Task;
use nokkvi_data::{backend::app_service::AppService, types::paged_buffer::PagedBuffer};
use tracing::{debug, error};

use super::components::{
    PaginatedFetch, prefetch_album_artwork_tasks, prefetch_song_artwork_tasks,
};
use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, CollageTarget, Message},
    views,
    widgets::{SlotListPageState, SlotListView, view_header::SortMode},
};

/// Per-view hooks driving `handle_loaded_with` and `handle_page_loaded_with`.
///
/// All methods are associated functions (no `&self`) so the generic body can
/// call them sequentially without double-borrowing `&mut Nokkvi`.
pub(crate) trait LoaderTarget {
    type Item: Send + 'static;

    // ── Required: library buffer ──────────────────────────────────────────
    fn library(app: &Nokkvi) -> &PagedBuffer<Self::Item>;
    fn library_mut(app: &mut Nokkvi) -> &mut PagedBuffer<Self::Item>;
    fn count_mut(app: &mut Nokkvi) -> &mut usize;

    // ── Required: page state ──────────────────────────────────────────────
    fn slot_list_mut(app: &mut Nokkvi) -> &mut SlotListView;

    // ── Required: artwork ─────────────────────────────────────────────────
    fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>>;
    fn center_large_artwork_task(app: &mut Nokkvi) -> Option<Task<Message>>;

    // ── Required: pending expand ──────────────────────────────────────────
    /// `None` for views with no pending-expand chain (Playlists).
    fn try_resolve_pending_expand(app: &mut Nokkvi) -> Option<Task<Message>>;

    // ── Required: identity ────────────────────────────────────────────────
    fn entity_label() -> &'static str;
    /// Extract the anchor-lookup id. Called by `apply_viewport_on_load` default impl.
    fn item_id(item: &Self::Item) -> &str;

    // ── Defaulted: viewport ───────────────────────────────────────────────
    /// Fallback when the anchor id is not found in the newly-loaded buffer.
    /// Default: reset to 0. `AlbumsTarget` overrides to clamp to `new_len − 1`
    /// so that if the centered item was deleted during a background sync the
    /// viewport stays near its last position instead of jumping to the top.
    fn anchor_miss_fallback(current_offset: usize, new_len: usize) -> usize {
        let _ = (current_offset, new_len);
        0
    }

    /// Apply viewport state after initial load. Default implements anchor-based
    /// preservation for paged views (Albums / Artists / Songs): on a background
    /// reload the centered item's id is used to relocate the viewport, keeping
    /// the user's position in large libraries. `GenresTarget` and
    /// `PlaylistsTarget` override to reset to 0 unconditionally because these
    /// are small, fully-refreshed lists where a background sync may reorder
    /// entries — preserving an arbitrary offset would be misleading.
    fn apply_viewport_on_load(app: &mut Nokkvi, background: bool, anchor_id: Option<&str>) {
        let new_len = Self::library(app).len();
        if !background {
            let sl = Self::slot_list_mut(app);
            sl.viewport_offset = 0;
            sl.selected_indices.clear();
            sl.anchor_index = None;
        } else {
            let current = Self::slot_list_mut(app).viewport_offset;
            let anchor_idx = anchor_id.and_then(|id| {
                Self::library(app)
                    .iter()
                    .position(|item| Self::item_id(item) == id)
            });
            let new_offset =
                anchor_idx.unwrap_or_else(|| Self::anchor_miss_fallback(current, new_len));
            let sl = Self::slot_list_mut(app);
            sl.viewport_offset = new_offset;
            sl.selected_offset = None;
            // Clear the multi-selection rather than retaining in-range indices:
            // `set_first_page` wholesale-replaces the buffer, so retained
            // absolute indices would point at DIFFERENT items after a
            // reorder / membership change, and a later positional batch op
            // would silently target the wrong songs. Matches the foreground
            // branch and the queue precedent (gotchas.md). The anchor-id
            // VIEWPORT relocation above is preserved.
            sl.selected_indices.clear();
            sl.anchor_index = None;
        }
    }

    // ── Defaulted: error-path cancel ──────────────────────────────────────
    /// Whether to call `cancel_pending_expand()` in the Err branch.
    /// Default: `true` (Albums / Artists / Songs / Genres). `PlaylistsTarget`
    /// overrides to `false` because playlists have no pending-expand chain
    /// (`try_resolve_pending_expand` always returns `None`) so there is nothing
    /// to cancel and the call would be a no-op.
    const CANCEL_PENDING_ON_ERR: bool = true;

    // ── Defaulted: post-load hook ─────────────────────────────────────────
    /// Called after `set_first_page` and viewport reset, before artwork dispatch.
    /// Default: no-op. `PlaylistsTarget` overrides to refresh the default-playlist picker.
    fn post_load_ok_hook(_app: &mut Nokkvi) {}

    // ── Required: page common state ───────────────────────────────────────
    /// Borrow the page's `SlotListPageState` (search / sort / scroll). Used by
    /// `Nokkvi::load_paged` to build `PaginatedFetch` params and read
    /// `viewport_offset` for the defensive `needs_fetch` gate. All five
    /// `*Target`s implement this even though only the three paged ones
    /// (Albums / Artists / Songs) call `load_paged`; keeping it on the parent
    /// trait avoids a sub-trait whose only members would be the same three.
    fn page_common(app: &Nokkvi) -> &SlotListPageState;

    /// Map a `SortMode` to the Subsonic API `type=` string for this entity.
    /// Used by `Nokkvi::load_paged` to build `PaginatedFetch::view_str`.
    fn sort_mode_to_api(mode: SortMode) -> &'static str;
}

// ── AlbumsTarget ─────────────────────────────────────────────────────────────

pub(crate) struct AlbumsTarget;

impl LoaderTarget for AlbumsTarget {
    type Item = nokkvi_data::backend::albums::AlbumUIViewData;

    fn library(app: &Nokkvi) -> &PagedBuffer<Self::Item> {
        &app.library.albums
    }

    fn library_mut(app: &mut Nokkvi) -> &mut PagedBuffer<Self::Item> {
        &mut app.library.albums
    }

    fn count_mut(app: &mut Nokkvi) -> &mut usize {
        &mut app.library.counts.albums
    }

    fn slot_list_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.albums_page.common.slot_list
    }

    fn item_id(item: &Self::Item) -> &str {
        &item.id
    }

    fn entity_label() -> &'static str {
        "Albums"
    }

    // Stay near current position when the anchor album was deleted.
    fn anchor_miss_fallback(current_offset: usize, new_len: usize) -> usize {
        current_offset.min(new_len.saturating_sub(1))
    }

    fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>> {
        let Some(shell) = &app.app_service else {
            return vec![];
        };
        let cached: HashSet<&String> = app.artwork.album_art.iter().map(|(k, _)| k).collect();
        prefetch_album_artwork_tasks(
            &app.albums_page.common.slot_list,
            &app.library.albums,
            &cached,
            &app.artwork.album_art_versions,
            shell.albums().clone(),
            |album| {
                (
                    album.id.clone(),
                    album.updated_at.clone(),
                    album.artwork_url.clone(),
                )
            },
        )
    }

    fn center_large_artwork_task(app: &mut Nokkvi) -> Option<Task<Message>> {
        let total = app.library.albums.len();
        let center_idx = app
            .albums_page
            .common
            .slot_list
            .get_center_item_index(total)?;
        let album_id = app.library.albums.get(center_idx)?.id.clone();
        Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
            album_id,
        ))))
    }

    fn try_resolve_pending_expand(app: &mut Nokkvi) -> Option<Task<Message>> {
        app.try_resolve_pending_expand_album()
    }

    fn page_common(app: &Nokkvi) -> &SlotListPageState {
        &app.albums_page.common
    }

    fn sort_mode_to_api(mode: SortMode) -> &'static str {
        views::AlbumsPage::sort_mode_to_api_string(mode)
    }
}

// ── ArtistsTarget ────────────────────────────────────────────────────────────

pub(crate) struct ArtistsTarget;

impl LoaderTarget for ArtistsTarget {
    type Item = nokkvi_data::backend::artists::ArtistUIViewData;

    fn library(app: &Nokkvi) -> &PagedBuffer<Self::Item> {
        &app.library.artists
    }

    fn library_mut(app: &mut Nokkvi) -> &mut PagedBuffer<Self::Item> {
        &mut app.library.artists
    }

    fn count_mut(app: &mut Nokkvi) -> &mut usize {
        &mut app.library.counts.artists
    }

    fn slot_list_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.artists_page.common.slot_list
    }

    fn item_id(item: &Self::Item) -> &str {
        &item.id
    }

    fn entity_label() -> &'static str {
        "Artists"
    }

    fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>> {
        if app.library.artists.is_empty() || app.app_service.is_none() {
            return vec![];
        }
        vec![app.prefetch_artist_mini_artwork_tasks()]
    }

    fn center_large_artwork_task(app: &mut Nokkvi) -> Option<Task<Message>> {
        let total = app.library.artists.len();
        if total == 0 || app.app_service.is_none() {
            return None;
        }
        let center_idx = app
            .artists_page
            .common
            .slot_list
            .get_center_item_index(total)?;
        let artist_id = app.library.artists.get(center_idx)?.id.clone();
        Some(app.handle_load_artist_large_artwork(artist_id))
    }

    fn try_resolve_pending_expand(app: &mut Nokkvi) -> Option<Task<Message>> {
        app.try_resolve_pending_expand_artist()
    }

    fn page_common(app: &Nokkvi) -> &SlotListPageState {
        &app.artists_page.common
    }

    fn sort_mode_to_api(mode: SortMode) -> &'static str {
        views::ArtistsPage::sort_mode_to_api_string(mode)
    }
}

// ── SongsTarget ──────────────────────────────────────────────────────────────

pub(crate) struct SongsTarget;

impl LoaderTarget for SongsTarget {
    type Item = nokkvi_data::backend::songs::SongUIViewData;

    fn library(app: &Nokkvi) -> &PagedBuffer<Self::Item> {
        &app.library.songs
    }

    fn library_mut(app: &mut Nokkvi) -> &mut PagedBuffer<Self::Item> {
        &mut app.library.songs
    }

    fn count_mut(app: &mut Nokkvi) -> &mut usize {
        &mut app.library.counts.songs
    }

    fn slot_list_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.songs_page.common.slot_list
    }

    fn item_id(item: &Self::Item) -> &str {
        &item.id
    }

    fn entity_label() -> &'static str {
        "Songs"
    }

    fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>> {
        let Some(shell) = &app.app_service else {
            return vec![];
        };
        let cached: HashSet<&String> = app.artwork.album_art.iter().map(|(k, _)| k).collect();
        prefetch_song_artwork_tasks(
            &app.songs_page.common.slot_list,
            &app.library.songs,
            &cached,
            &app.artwork.album_art_versions,
            shell.albums().clone(),
            |song| {
                song.album_id
                    .as_ref()
                    .map(|id| (id, song.updated_at.clone()))
            },
        )
    }

    fn center_large_artwork_task(app: &mut Nokkvi) -> Option<Task<Message>> {
        let total = app.library.songs.len();
        let center_idx = app
            .songs_page
            .common
            .slot_list
            .get_center_item_index(total)?;
        let album_id = app
            .library
            .songs
            .get(center_idx)?
            .album_id
            .as_ref()?
            .clone();
        Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(
            album_id,
        ))))
    }

    fn try_resolve_pending_expand(app: &mut Nokkvi) -> Option<Task<Message>> {
        app.try_resolve_pending_expand_song()
    }

    fn page_common(app: &Nokkvi) -> &SlotListPageState {
        &app.songs_page.common
    }

    fn sort_mode_to_api(mode: SortMode) -> &'static str {
        views::SongsPage::sort_mode_to_api_string(mode)
    }
}

// ── GenresTarget ─────────────────────────────────────────────────────────────

pub(crate) struct GenresTarget;

impl LoaderTarget for GenresTarget {
    type Item = nokkvi_data::backend::genres::GenreUIViewData;

    fn library(app: &Nokkvi) -> &PagedBuffer<Self::Item> {
        &app.library.genres
    }

    fn library_mut(app: &mut Nokkvi) -> &mut PagedBuffer<Self::Item> {
        &mut app.library.genres
    }

    fn count_mut(app: &mut Nokkvi) -> &mut usize {
        &mut app.library.counts.genres
    }

    fn slot_list_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.genres_page.common.slot_list
    }

    fn item_id(item: &Self::Item) -> &str {
        &item.id
    }

    fn entity_label() -> &'static str {
        "Genres"
    }

    // Genres are a small, fully-refreshed list — always start at 0.
    fn apply_viewport_on_load(app: &mut Nokkvi, _background: bool, _anchor_id: Option<&str>) {
        Self::slot_list_mut(app).viewport_offset = 0;
    }

    fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>> {
        let mut tasks = Vec::new();
        tasks.push(Task::done(Message::Artwork(
            ArtworkMessage::StartCollagePrefetch(CollageTarget::Genre),
        )));
        if !app.library.genres.is_empty() {
            tasks.push(Task::done(Message::Genres(views::GenresMessage::SlotList(
                crate::widgets::SlotListPageMessage::SetOffset(
                    0,
                    iced::keyboard::Modifiers::default(),
                ),
            ))));
        }
        tasks
    }

    fn center_large_artwork_task(_app: &mut Nokkvi) -> Option<Task<Message>> {
        None
    }

    fn try_resolve_pending_expand(app: &mut Nokkvi) -> Option<Task<Message>> {
        app.try_resolve_pending_expand_genre()
    }

    fn page_common(app: &Nokkvi) -> &SlotListPageState {
        &app.genres_page.common
    }

    fn sort_mode_to_api(mode: SortMode) -> &'static str {
        views::GenresPage::sort_mode_to_api_string(mode)
    }
}

// ── PlaylistsTarget ──────────────────────────────────────────────────────────

pub(crate) struct PlaylistsTarget;

impl LoaderTarget for PlaylistsTarget {
    type Item = nokkvi_data::backend::playlists::PlaylistUIViewData;

    fn library(app: &Nokkvi) -> &PagedBuffer<Self::Item> {
        &app.library.playlists
    }

    fn library_mut(app: &mut Nokkvi) -> &mut PagedBuffer<Self::Item> {
        &mut app.library.playlists
    }

    fn count_mut(app: &mut Nokkvi) -> &mut usize {
        &mut app.library.counts.playlists
    }

    fn slot_list_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.playlists_page.common.slot_list
    }

    fn item_id(item: &Self::Item) -> &str {
        &item.id
    }

    fn entity_label() -> &'static str {
        "Playlists"
    }

    const CANCEL_PENDING_ON_ERR: bool = false;

    // Playlists are a small, fully-refreshed list — always start at 0.
    fn apply_viewport_on_load(app: &mut Nokkvi, _background: bool, _anchor_id: Option<&str>) {
        Self::slot_list_mut(app).viewport_offset = 0;
    }

    fn post_load_ok_hook(app: &mut Nokkvi) {
        app.refresh_default_playlist_picker_after_load();
    }

    fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>> {
        let mut tasks = Vec::new();
        tasks.push(Task::done(Message::Artwork(
            ArtworkMessage::StartCollagePrefetch(CollageTarget::Playlist),
        )));
        if !app.library.playlists.is_empty() {
            tasks.push(Task::done(Message::Playlists(
                views::PlaylistsMessage::SlotList(crate::widgets::SlotListPageMessage::SetOffset(
                    0,
                    iced::keyboard::Modifiers::default(),
                )),
            )));
        }
        tasks
    }

    fn center_large_artwork_task(_app: &mut Nokkvi) -> Option<Task<Message>> {
        None
    }

    fn try_resolve_pending_expand(_app: &mut Nokkvi) -> Option<Task<Message>> {
        None
    }

    fn page_common(app: &Nokkvi) -> &SlotListPageState {
        &app.playlists_page.common
    }

    fn sort_mode_to_api(mode: SortMode) -> &'static str {
        views::PlaylistsPage::sort_mode_to_api_string(mode)
    }
}

impl Nokkvi {
    /// Shared paginated-fetch dispatch for the three paged library views
    /// (Albums, Artists, Songs). Owns the pre-fetch invariant body so each
    /// per-view `handle_load_*` / `handle_*_load_page` / `force_load_*_page`
    /// can collapse to a one-line delegation.
    ///
    /// The body, in order:
    /// 1. Read `library_page_size` from settings.
    /// 2. **Phase 5A defensive gate** — when `force == false` and `offset > 0`,
    ///    skip the dispatch if `PagedBuffer::needs_fetch` returns `None`.
    ///    This catches duplicate dispatches that race past the upstream
    ///    `needs_fetch` check at the action site. Initial loads (`offset == 0`)
    ///    always proceed — sort/search changes need a fresh page even if the
    ///    old one is still in flight.
    /// 3. Build `PaginatedFetch` from the page's common state and the per-
    ///    entity sort mapper (`T::sort_mode_to_api`).
    /// 4. Emit a `LoadXxx` debug log keyed by `T::entity_label()`.
    /// 5. **`set_loading(true)`** on the entity's `PagedBuffer` BEFORE
    ///    dispatching — codifies the CLAUDE.md "always call `set_loading(true)`
    ///    before dispatching a page fetch" invariant so rapid scroll cannot
    ///    trigger duplicate fetches.
    /// 6. `shell_task` with the caller-supplied `fetch` closure. The closure
    ///    receives the prepared `PaginatedFetch` and the `AppService` shell,
    ///    and is responsible for the per-entity backend call + UI-projection
    ///    mapping. Per-call state that varies between calls of the same entity
    ///    (e.g. Artists' `album_artists_only`, the rating-sort flag) is
    ///    captured by the closure at the call site, not threaded through the
    ///    trait — keeps Albums/Songs fetch signatures clean.
    pub(crate) fn load_paged<T, F, Fut, M>(
        &mut self,
        offset: usize,
        force: bool,
        msg_ctor: M,
        fetch: F,
    ) -> Task<Message>
    where
        T: LoaderTarget,
        F: FnOnce(AppService, PaginatedFetch) -> Fut + Send + 'static,
        Fut: Future<Output = (Result<Vec<T::Item>, String>, usize)> + Send + 'static,
        M: FnOnce((Result<Vec<T::Item>, String>, usize)) -> Message + Send + 'static,
    {
        let page_size = self.settings.library_page_size.to_usize();
        let viewport_offset = T::page_common(self).slot_list.viewport_offset;
        // Phase 5A defensive gate: page-load follow-ups (offset > 0) must
        // pass needs_fetch. Catches duplicate dispatches that race past
        // the upstream needs_fetch check at the action site. Initial
        // loads (offset 0) always proceed — sort/search changes need a
        // fresh page even if the old one is still in flight.
        if !force
            && offset > 0
            && T::library(self)
                .needs_fetch(viewport_offset, page_size)
                .is_none()
        {
            return Task::none();
        }
        let params = PaginatedFetch::from_common(
            T::page_common(self),
            T::sort_mode_to_api,
            offset,
            page_size,
        );
        debug!(
            " Load{}: offset={}, page_size={}, view={}, sort={}, search={:?}",
            T::entity_label(),
            params.offset,
            params.page_size,
            params.view_str,
            params.sort_order,
            params.search_query,
        );

        T::library_mut(self).set_loading(true);

        self.shell_task(move |shell| fetch(shell, params), msg_ctor)
    }

    /// Re-pin the slot-list highlight after a `set_children`-triggering load
    /// (Albums `TracksLoaded`, Artists/Genres `AlbumsLoaded`) when the load
    /// resolves the active find-and-expand chain target.
    ///
    /// The page's `update` clears `selected_offset` when it calls
    /// `set_children`, which would lose the find-chain's intended highlight
    /// position. This helper:
    /// 1. Checks `pending_top_pin` matches the supplied `loaded_id` and
    ///    `expected_kind`.
    /// 2. Looks up the entity index in `library` via `find_position`.
    /// 3. Computes the post-expansion flat-list length via `flattened_len`.
    /// 4. Calls `slot_list.pin_selected(idx, total)` to restore the
    ///    highlight.
    /// 5. Clears `pending_top_pin` so a later, unrelated load doesn't
    ///    re-fire the pin.
    ///
    /// The three callers (Albums, Artists, Genres) differ only in their
    /// `PendingTopPin` variant and their per-view library / expansion / page
    /// references — encoded by the caller's closures.
    pub(crate) fn pin_after_load<F1, F2, F3>(
        &mut self,
        loaded_id: &str,
        matches_pin: F1,
        find_position: F2,
        pin: F3,
    ) where
        F1: FnOnce(&crate::state::PendingTopPin, &str) -> bool,
        F2: FnOnce(&Self, &str) -> Option<usize>,
        F3: FnOnce(&mut Self, usize),
    {
        let Some(pin_state) = &self.pending_top_pin else {
            return;
        };
        if !matches_pin(pin_state, loaded_id) {
            return;
        }
        let Some(idx) = find_position(self, loaded_id) else {
            return;
        };
        pin(self, idx);
        self.pending_top_pin = None;
    }

    /// Shared "prefetch viewport mini artwork + chain a page-load if scrolling
    /// near the loaded edge" tail used by every paged view's
    /// `LoadLargeArtwork` action arm. Returns the batched tasks ready to be
    /// concatenated with the caller's site-specific large-artwork task.
    ///
    /// Callers are responsible for dispatching the per-site large-artwork
    /// task themselves (Albums uses expansion-aware id resolution, Artists
    /// derives the center artist, Songs takes the album_id directly) and
    /// must call this AFTER pushing that task into their `tasks` Vec.
    ///
    /// `T::prefetch_artwork_tasks` runs the per-entity viewport prefetch
    /// (skipping cached ids); the `needs_fetch` chain calls each entity's
    /// per-view `handle_X_load_page` only when the scroll has crossed the
    /// loaded edge.
    pub(crate) fn prefetch_and_maybe_load_next_page<T: LoaderTarget>(
        &mut self,
        load_page: impl FnOnce(&mut Self, usize) -> Task<Message>,
    ) -> Vec<Task<Message>> {
        let mut tasks: Vec<Task<Message>> = T::prefetch_artwork_tasks(self);

        let page_size = self.settings.library_page_size.to_usize();
        let viewport_offset = T::page_common(self).slot_list.viewport_offset;
        if !T::library(self).is_empty()
            && let Some((offset, _)) = T::library(self).needs_fetch(viewport_offset, page_size)
        {
            tasks.push(load_page(self, offset));
        }
        tasks
    }

    pub(crate) fn handle_loaded_with<T: LoaderTarget>(
        &mut self,
        result: Result<Vec<T::Item>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
    ) -> Task<Message> {
        *T::count_mut(self) = total_count;
        match result {
            Ok(items) => {
                debug!(
                    "✅ Loaded {} {} (total: {})",
                    items.len(),
                    T::entity_label(),
                    total_count
                );
                T::library_mut(self).set_first_page(items, total_count);
                T::apply_viewport_on_load(self, background, anchor_id.as_deref());
                T::post_load_ok_hook(self);

                let mut tasks: Vec<Task<Message>> = T::prefetch_artwork_tasks(self);
                if let Some(task) = T::center_large_artwork_task(self) {
                    tasks.push(task);
                }
                if let Some(task) = T::try_resolve_pending_expand(self) {
                    tasks.push(task);
                }
                if !tasks.is_empty() {
                    return Task::batch(tasks);
                }
            }
            Err(e) => {
                if e.contains("Unauthorized") {
                    T::library_mut(self).set_loading(false);
                    return self.handle_session_expired();
                }
                error!("Error loading {}: {}", T::entity_label(), e);
                T::library_mut(self).set_loading(false);
                if T::CANCEL_PENDING_ON_ERR {
                    self.cancel_pending_expand();
                }
                self.toast_error(format!("Failed to load {}: {e}", T::entity_label()));
            }
        }
        Task::none()
    }

    pub(crate) fn handle_page_loaded_with<T: LoaderTarget>(
        &mut self,
        result: Result<Vec<T::Item>, String>,
        total_count: usize,
    ) -> Task<Message> {
        match result {
            Ok(new_items) => {
                let count = new_items.len();
                let loaded_before = T::library(self).loaded_count();
                T::library_mut(self).append_page(new_items, total_count);
                debug!(
                    "📄 {} page loaded: {} new items ({}→{} of {})",
                    T::entity_label(),
                    count,
                    loaded_before,
                    T::library(self).loaded_count(),
                    total_count,
                );
                if let Some(task) = T::try_resolve_pending_expand(self) {
                    return task;
                }
            }
            Err(e) => {
                if e.contains("Unauthorized") {
                    T::library_mut(self).set_loading(false);
                    return self.handle_session_expired();
                }
                error!("Error loading {} page: {}", T::entity_label(), e);
                T::library_mut(self).set_loading(false);
                self.cancel_pending_expand();
                self.toast_error(format!("Failed to load {}: {e}", T::entity_label()));
            }
        }
        Task::none()
    }
}
