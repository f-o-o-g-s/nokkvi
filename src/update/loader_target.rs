//! Generic load-result handlers driven by per-view `LoaderTarget` specs.
//!
//! All five library views (Albums, Artists, Songs, Genres, Playlists) share
//! the same 9-step skeleton for handling initial loads and page appends.
//! `handle_loaded_with` and `handle_page_loaded_with` are the single bodies;
//! per-view differences live in the five zero-sized `*Target` marker structs.

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

// ── Marker structs (impls follow after both generic bodies) ──────────────────
pub(crate) struct AlbumsTarget;
pub(crate) struct ArtistsTarget;
pub(crate) struct SongsTarget;
pub(crate) struct GenresTarget;
pub(crate) struct PlaylistsTarget;

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
