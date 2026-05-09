//! Descriptor-driven generic body for the four
//! `try_resolve_pending_expand_*` find-and-expand resolvers.
//!
//! The four entity flavors (album / artist / genre / song) used to live as
//! parallel ~70-90 LOC bodies in `update/navigation.rs`. Per-entity quirks
//! (genre's name-vs-id match, song's center-only / no-FocusAndExpand,
//! genre's single-shot pagination) now become typed data on the per-spec
//! impls rather than divergence across four hand-mirrored function bodies.

use std::borrow::Cow;

use iced::Task;
use nokkvi_data::{
    backend::{
        albums::AlbumUIViewData, artists::ArtistUIViewData, genres::GenreUIViewData,
        songs::SongUIViewData,
    },
    types::paged_buffer::PagedBuffer,
};
use tracing::{debug, warn};

use crate::{
    Nokkvi,
    app_message::Message,
    state::{LibraryData, PendingExpand, PendingTopPin},
    views,
    widgets::SlotListView,
};

/// Per-entity hooks driving the generic [`Nokkvi::try_resolve_pending_expand_with`] body.
///
/// The four implementations ([`AlbumSpec`], [`ArtistSpec`], [`GenreSpec`],
/// [`SongSpec`]) are zero-sized types — the trait is a vtable for the
/// entity's quirks, not runtime polymorphism.
pub(crate) trait ResolveSpec {
    /// Library entity (e.g. `AlbumUIViewData`).
    type Item;

    /// Pull the target id from a [`PendingExpand`] if it matches this spec's variant.
    fn target_id(pending: &PendingExpand) -> Option<String>;

    /// The library buffer this resolver scans.
    fn library(library: &LibraryData) -> &PagedBuffer<Self::Item>;

    /// The slot-list widget on the corresponding page (`*_page.common.slot_list`).
    fn page_mut(app: &mut Nokkvi) -> &mut SlotListView;

    /// Predicate matching one library item against the resolver's target id.
    /// Genre overrides this to match by `name`; others match by `id`. Returns
    /// the *resolved* id to pin — for genres the resolved id is the internal
    /// UUID, distinct from the matched display name.
    fn match_target<'a>(item: &'a Self::Item, target: &'a str) -> Option<Cow<'a, str>>;

    /// Build the per-entity FocusAndExpand message. Returns `None` for Song
    /// (songs are not expandable; the resolver only centers).
    fn focus_and_expand(idx: usize) -> Option<Message>;

    /// Wrap the resolved id in a [`PendingTopPin`] variant. Returns `None` for Song.
    fn pin(resolved_id: String) -> Option<PendingTopPin>;

    /// Force-load the next page when the target isn't in the buffer yet.
    /// Returns `None` for Genre (single-shot — no more pages will arrive).
    fn force_load_next_page(app: &mut Nokkvi, offset: usize) -> Option<Task<Message>>;

    /// Toast text and warn-log label, e.g. `"Album"` → `"Album not found in library"`.
    fn label() -> &'static str;
}

pub(crate) struct AlbumSpec;
#[expect(
    dead_code,
    reason = "wired up by try_resolve_pending_expand_artist wrapper migration"
)]
pub(crate) struct ArtistSpec;
#[expect(
    dead_code,
    reason = "wired up by try_resolve_pending_expand_genre wrapper migration"
)]
pub(crate) struct GenreSpec;
#[expect(
    dead_code,
    reason = "wired up by try_resolve_pending_expand_song wrapper migration"
)]
pub(crate) struct SongSpec;

impl ResolveSpec for AlbumSpec {
    type Item = AlbumUIViewData;

    fn target_id(pending: &PendingExpand) -> Option<String> {
        match pending {
            PendingExpand::Album { album_id, .. } => Some(album_id.clone()),
            _ => None,
        }
    }

    fn library(library: &LibraryData) -> &PagedBuffer<Self::Item> {
        &library.albums
    }

    fn page_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.albums_page.common.slot_list
    }

    fn match_target<'a>(item: &'a Self::Item, target: &'a str) -> Option<Cow<'a, str>> {
        (item.id == target).then_some(Cow::Borrowed(target))
    }

    fn focus_and_expand(idx: usize) -> Option<Message> {
        Some(Message::Albums(views::AlbumsMessage::FocusAndExpand(idx)))
    }

    fn pin(resolved_id: String) -> Option<PendingTopPin> {
        Some(PendingTopPin::Album(resolved_id))
    }

    fn force_load_next_page(app: &mut Nokkvi, offset: usize) -> Option<Task<Message>> {
        Some(app.force_load_albums_page(offset))
    }

    fn label() -> &'static str {
        "Album"
    }
}

impl ResolveSpec for ArtistSpec {
    type Item = ArtistUIViewData;

    fn target_id(pending: &PendingExpand) -> Option<String> {
        match pending {
            PendingExpand::Artist { artist_id, .. } => Some(artist_id.clone()),
            _ => None,
        }
    }

    fn library(library: &LibraryData) -> &PagedBuffer<Self::Item> {
        &library.artists
    }

    fn page_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.artists_page.common.slot_list
    }

    fn match_target<'a>(item: &'a Self::Item, target: &'a str) -> Option<Cow<'a, str>> {
        (item.id == target).then_some(Cow::Borrowed(target))
    }

    fn focus_and_expand(idx: usize) -> Option<Message> {
        Some(Message::Artists(views::ArtistsMessage::FocusAndExpand(idx)))
    }

    fn pin(resolved_id: String) -> Option<PendingTopPin> {
        Some(PendingTopPin::Artist(resolved_id))
    }

    fn force_load_next_page(app: &mut Nokkvi, offset: usize) -> Option<Task<Message>> {
        Some(app.force_load_artists_page(offset))
    }

    fn label() -> &'static str {
        "Artist"
    }
}

impl ResolveSpec for GenreSpec {
    type Item = GenreUIViewData;

    fn target_id(pending: &PendingExpand) -> Option<String> {
        match pending {
            PendingExpand::Genre { genre_id, .. } => Some(genre_id.clone()),
            _ => None,
        }
    }

    fn library(library: &LibraryData) -> &PagedBuffer<Self::Item> {
        &library.genres
    }

    fn page_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.genres_page.common.slot_list
    }

    /// Genre's quirk: Navidrome's `/api/genre` returns internal UUIDs that
    /// differ from the display names; click sites only have access to the
    /// display name. The lookup matches against `item.name`, but the
    /// `pending_top_pin` and downstream `AlbumsLoaded(genre_id, …)` messages
    /// carry the resolved internal UUID — that's what `Cow::Owned(item.id)`
    /// surfaces back to the generic body.
    fn match_target<'a>(item: &'a Self::Item, target: &'a str) -> Option<Cow<'a, str>> {
        (item.name == target).then(|| Cow::Owned(item.id.clone()))
    }

    fn focus_and_expand(idx: usize) -> Option<Message> {
        Some(Message::Genres(views::GenresMessage::FocusAndExpand(idx)))
    }

    fn pin(resolved_id: String) -> Option<PendingTopPin> {
        Some(PendingTopPin::Genre(resolved_id))
    }

    /// Genre is single-shot — `/api/genre` returns the entire list in one
    /// page, so a not-found-and-idle outcome is terminal. The generic body
    /// observes the `None` and emits the warn + toast itself.
    fn force_load_next_page(_app: &mut Nokkvi, _offset: usize) -> Option<Task<Message>> {
        None
    }

    fn label() -> &'static str {
        "Genre"
    }
}

impl ResolveSpec for SongSpec {
    type Item = SongUIViewData;

    fn target_id(pending: &PendingExpand) -> Option<String> {
        match pending {
            PendingExpand::Song { song_id, .. } => Some(song_id.clone()),
            _ => None,
        }
    }

    fn library(library: &LibraryData) -> &PagedBuffer<Self::Item> {
        &library.songs
    }

    fn page_mut(app: &mut Nokkvi) -> &mut SlotListView {
        &mut app.songs_page.common.slot_list
    }

    fn match_target<'a>(item: &'a Self::Item, target: &'a str) -> Option<Cow<'a, str>> {
        (item.id == target).then_some(Cow::Borrowed(target))
    }

    /// Songs aren't expandable. Returning `None` here makes the generic body
    /// behave as if `pending_expand_center_only` were set: viewport_offset is
    /// `idx` directly (not `idx + center_slot`), `pending_top_pin` stays
    /// untouched, and only the prefetch task is returned.
    fn focus_and_expand(_idx: usize) -> Option<Message> {
        None
    }

    fn pin(_resolved_id: String) -> Option<PendingTopPin> {
        None
    }

    fn force_load_next_page(app: &mut Nokkvi, offset: usize) -> Option<Task<Message>> {
        Some(app.force_load_songs_page(offset))
    }

    fn label() -> &'static str {
        "Song"
    }
}

impl Nokkvi {
    /// Generic body for the four `try_resolve_pending_expand_*` resolvers.
    ///
    /// Five disjoint exits — found+focus, found+center, fully-loaded miss,
    /// loading (retry later), idle+force-load (or single-shot terminal miss).
    /// One body so the state machine stays visible; per-entity quirks are
    /// supplied by the [`ResolveSpec`] implementation.
    pub(crate) fn try_resolve_pending_expand_with<S: ResolveSpec>(
        &mut self,
    ) -> Option<Task<Message>> {
        let target_id = S::target_id(self.pending_expand.as_ref()?)?;

        let found = S::library(&self.library)
            .iter()
            .enumerate()
            .find_map(|(i, item)| {
                S::match_target(item, &target_id).map(|resolved| (i, resolved.into_owned()))
            });

        let label = S::label();
        if let Some((idx, resolved_id)) = found {
            let center_only = self.pending_expand_center_only;
            let focus_msg = S::focus_and_expand(idx);
            // Effective center-only: the explicit flag, OR the spec doesn't
            // dispatch a FocusAndExpand at all (Song). In either case we
            // place the target at the center slot and skip the top pin.
            let effective_center_only = center_only || focus_msg.is_none();
            debug!(
                " [EXPAND] Found {} '{}' at index {} — {}",
                label,
                target_id,
                idx,
                if effective_center_only {
                    "centering (CenterOnPlaying)"
                } else {
                    "scrolling + dispatching FocusAndExpand"
                }
            );
            self.pending_expand = None;
            self.pending_expand_center_only = false;
            let total = S::library(&self.library).len();
            let page = S::page_mut(self);
            // Click chain pins to slot 0 (so the expanded children fill the
            // viewport below); CenterOnPlaying centers the row and skips
            // expansion. viewport_offset is the index rendered at the
            // *center slot*, so `idx + center_slot` shifts the window down
            // by `center_slot`, leaving the target at slot 0.
            let target_offset = if effective_center_only {
                idx
            } else {
                let center_slot = page.slot_count.max(2) / 2;
                idx.saturating_add(center_slot).min(total.saturating_sub(1))
            };
            page.set_offset(target_offset, total);
            // set_offset clears selected_offset; re-set as a top-pin so the
            // target keeps the highlight styling (effective center derives
            // from selected_offset before falling back to viewport_offset)
            // AND the next mouse-wheel scroll doesn't snap the viewport
            // backward to `idx`.
            page.pin_selected(idx, total);
            page.flash_center();
            // Mini-artwork prefetch follows the viewport. The page-load
            // prefetch ran for viewport=0 (and page-2/3 loads don't prefetch
            // at all), so the rows around the new viewport would render
            // as empty placeholders without an explicit kick here.
            let prefetch_task = self.prefetch_viewport_artwork();
            return Some(match (effective_center_only, focus_msg) {
                (false, Some(focus_msg)) => {
                    // Pin the highlight onto the target so it survives
                    // `set_children` when children land — the per-view
                    // post-hook (TracksLoaded / AlbumsLoaded) re-runs
                    // set_selected for this id.
                    self.pending_top_pin = S::pin(resolved_id);
                    Task::batch([prefetch_task, Task::done(focus_msg)])
                }
                _ => prefetch_task,
            });
        }

        if S::library(&self.library).fully_loaded() {
            warn!(
                " [EXPAND] {} '{}' not found after full load — clearing target",
                label, target_id
            );
            self.toast_warn(format!("{label} not found in library"));
            self.pending_expand = None;
            self.pending_expand_center_only = false;
            return Some(Task::none());
        }

        if S::library(&self.library).is_loading() {
            return None;
        }

        let next_offset = S::library(&self.library).loaded_count();
        if let Some(task) = S::force_load_next_page(self, next_offset) {
            debug!(
                " [EXPAND] {} '{}' not in buffer — force-fetching next page at offset {}",
                label, target_id, next_offset
            );
            return Some(task);
        }

        // Single-shot (Genre): idle + not-found is terminal. No more pages
        // will arrive, so warn + toast + clear here rather than waiting.
        warn!(
            " [EXPAND] {} '{}' not found after load — clearing target",
            label, target_id
        );
        self.toast_warn(format!("{label} not found in library"));
        self.pending_expand = None;
        self.pending_expand_center_only = false;
        Some(Task::none())
    }
}
