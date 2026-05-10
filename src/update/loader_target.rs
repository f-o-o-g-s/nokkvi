//! Generic load-result handlers driven by per-view `LoaderTarget` specs.
//!
//! All five library views (Albums, Artists, Songs, Genres, Playlists) share
//! the same 9-step skeleton for handling initial loads and page appends.
//! `handle_loaded_with` and `handle_page_loaded_with` are the single bodies;
//! per-view differences live in the five zero-sized `*Target` marker structs.
//!
//! `dead_code` is suppressed at module level because no callers exist until
//! Lanes B and C migrate the existing handler bodies to delegate here.
#![allow(dead_code)]

use std::collections::HashSet;

use iced::Task;
use nokkvi_data::types::paged_buffer::PagedBuffer;
use tracing::{debug, error};

use super::components::{prefetch_album_artwork_tasks, prefetch_song_artwork_tasks};
use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, CollageTarget, Message},
    views,
    widgets::SlotListView,
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
    /// Default: reset to 0. `AlbumsTarget` overrides to clamp to `new_len − 1`.
    fn anchor_miss_fallback(current_offset: usize, new_len: usize) -> usize {
        let _ = (current_offset, new_len);
        0
    }

    /// Apply viewport state after initial load. Default implements the paged
    /// behavior (Albums / Artists / Songs). `GenresTarget` and `PlaylistsTarget`
    /// override to reset to 0 unconditionally.
    fn apply_viewport_on_load(app: &mut Nokkvi, background: bool, anchor_id: Option<&str>) {
        let new_len = Self::library(app).len();
        if !background {
            let sl = Self::slot_list_mut(app);
            sl.viewport_offset = 0;
            sl.selected_indices.clear();
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
            sl.selected_indices.retain(|&i| i < new_len);
        }
    }

    // ── Defaulted: error-path cancel ──────────────────────────────────────
    /// Whether to call `cancel_pending_expand()` in the Err branch.
    /// Default: `true`. `PlaylistsTarget` overrides to `false` (no pending expand).
    const CANCEL_PENDING_ON_ERR: bool = true;

    // ── Defaulted: post-load hook ─────────────────────────────────────────
    /// Called after `set_first_page` and viewport reset, before artwork dispatch.
    /// Default: no-op. `PlaylistsTarget` overrides to refresh the default-playlist picker.
    fn post_load_ok_hook(_app: &mut Nokkvi) {}
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
            shell.albums().clone(),
            |album| (album.id.clone(), album.artwork_url.clone()),
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
            shell.albums().clone(),
            |song| song.album_id.as_ref(),
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
}

impl Nokkvi {
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
                    "✅ Loaded {} {}s (total: {})",
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
