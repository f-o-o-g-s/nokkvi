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

use std::future::Future;

use iced::Task;
use nokkvi_data::{
    backend::app_service::AppService,
    types::{
        paged_buffer::{PageSlot, PagedBuffer, ReplaceTicket},
        song::SongCover,
    },
};
use tracing::{debug, error};

use super::components::{
    ArtworkGate, PaginatedFetch, album_prefetch_entry, prefetch_album_artwork_tasks,
    prefetch_song_artwork_tasks, song_cover_entry,
};
use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, CollageTarget, Message},
    views,
    widgets::{SlotListPageState, SlotListView, view_header::SortMode},
};

/// What a `Nokkvi::load_paged` call fetches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PagedLoad {
    /// One page from the top: a fresh list (view entry, sort, search,
    /// filter).
    First,
    /// A background reload's rows from the top ([`Reload`]).
    Reload(usize),
    /// The page at this offset, as the scroll nears the loaded edge;
    /// skipped when `needs_fetch` finds nothing due.
    Page(usize),
    /// The page at this offset for a find chain, past the `needs_fetch`
    /// gate.
    ForcedPage(usize),
}

/// A paged view's background reload (`handle_library_changed`): `rows`
/// from the top ([`reload_rows`]), and the row the viewport was on, which
/// `apply_viewport_on_load` finds again once they land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reload {
    pub(crate) rows: usize,
    pub(crate) anchor_id: Option<String>,
}

impl Reload {
    /// A list's first load: the reload's rows with its anchor, else one
    /// fresh page.
    pub(crate) fn first_rows(reload: Option<Self>) -> (PagedLoad, Option<String>) {
        match reload {
            Some(Self { rows, anchor_id }) => (PagedLoad::Reload(rows), anchor_id),
            None => (PagedLoad::First, None),
        }
    }
}

/// What a paged fetch hands its message constructor: the rows (or the
/// error), the server's total and, for a load that replaces the list (a
/// first load, a sort or search, a background reload), its ticket, which
/// `handle_loaded_with` checks before it applies the answer.
pub(crate) struct PagedOutcome<I> {
    pub result: Result<Vec<I>, String>,
    pub total_count: usize,
    pub replace: Option<ReplaceTicket>,
}

/// Most rows a background reload fetches. A view scrolled deeper keeps its
/// rows until the user reloads it (sort, search, refresh): refetching every
/// row above that deep a position on each scrobble would cost a second or
/// more and megabytes (5,000 songs took 0.45 s from a local server).
pub(crate) const MAX_RELOAD_ROWS: usize = 10_000;

/// Rows a background reload fetches from the top: down past the last
/// visible row and the fetch look-ahead below it, in whole pages, so the row
/// the viewport is on is in the new buffer and no page fetch is due once it
/// lands. One page reloaded only the top of a long list: a viewport further
/// down lost its row and went back to the top. `None` past
/// [`MAX_RELOAD_ROWS`].
pub(crate) fn reload_rows(slot_list: &SlotListView, page_size: usize) -> Option<usize> {
    let page_size = page_size.max(1);
    let through = slot_list.viewport_offset
        + slot_list.slot_count
        + nokkvi_data::types::player_settings::pagination_fetch_threshold(page_size);
    let rows = through.div_ceil(page_size).max(1) * page_size;
    (rows <= MAX_RELOAD_ROWS.max(page_size)).then_some(rows)
}

/// Per-view hooks driving `handle_loaded_with` and `handle_page_loaded_with`.
///
/// All methods are associated functions (no `&self`) so the generic body can
/// call them sequentially without double-borrowing `&mut Nokkvi`.
/// A highlighted row by identity rather than index ([`LoaderTarget::focused_row`]):
/// the id of its parent row in the library buffer, how many rows below that
/// parent it sits (an expanded child), and whether it is a find-and-expand pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FocusedRow {
    place: RowPlace,
    pinned: bool,
}

/// A row by identity rather than index, in terms a reload can find again:
/// its parent row's id and its place under that parent (0 for the parent
/// itself, n for its n-th expanded child). The slot indices are flattened
/// under an inline expansion, so an index into them never indexes the
/// parent list directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RowPlace {
    id: String,
    below: usize,
}

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
    /// Apply viewport state after initial load. Default implements anchor-based
    /// preservation for paged views (Albums / Artists / Songs): on a background
    /// reload the viewport's centred row (its anchor) is found again by id
    /// (the reload fetched every row down to it, `reload_rows`), keeping the user's position in
    /// large libraries; a row gone from the list leaves the viewport at the
    /// same offset, clamped to the new length. `GenresTarget` and
    /// `PlaylistsTarget` override to reset to 0 unconditionally because these
    /// are small, fully-refreshed lists where a background sync may reorder
    /// entries — preserving an arbitrary offset would be misleading.
    fn apply_viewport_on_load(app: &mut Nokkvi, background: bool, anchor_id: Option<&str>) {
        if !background {
            let sl = Self::slot_list_mut(app);
            sl.viewport_offset = 0;
            // The whole buffer is replaced and the viewport goes back to 0, so
            // the click-to-focus marker goes with the indices and the anchor:
            // it is an absolute index into the OLD rows, and
            // `get_effective_center_index` prefers it, so Enter / Shift+Q would
            // act on an off-screen row after a sort change. Same reset as the
            // background branch below.
            sl.clear_selection_for_refresh();
        } else {
            let current = Self::slot_list_mut(app).viewport_offset;
            let anchor_idx = anchor_id.and_then(|id| {
                Self::library(app)
                    .iter()
                    .position(|item| Self::item_id(item) == id)
            });
            // The clamp is against the rows shown, flattened under an
            // expansion (`reanchor` then lands the viewport exactly).
            let shown = Self::flattened_len(app);
            let new_offset = anchor_idx.unwrap_or_else(|| current.min(shown.saturating_sub(1)));
            let sl = Self::slot_list_mut(app);
            sl.viewport_offset = new_offset;
            // Clear the multi-selection rather than retaining in-range indices:
            // `set_first_page` wholesale-replaces the buffer, so retained
            // absolute indices would point at DIFFERENT items after a
            // reorder / membership change, and a later positional batch op
            // would silently target the wrong songs. Matches the foreground
            // branch and the queue precedent (gotchas/queue.md "Stale
            // multi-selection across refreshes"). The anchor-id VIEWPORT
            // relocation above is preserved. The highlighted row is then put
            // back by id (`refocus`, from `handle_loaded_with`), so a reload
            // the server's artist refresh triggers no longer drops the row a
            // jump just landed on.
            sl.clear_selection_for_refresh();
        }
    }

    // ── Defaulted: the highlighted row across a background reload ─────────
    /// The row flattened slot-list index `flat` shows, as (index in
    /// [`Self::library`], rows below that parent): 0 for the parent row, n for
    /// its n-th expanded child. `None` past the end. Default: a view without
    /// inline expansion, where the two indices are the same.
    fn flat_to_parent(app: &Nokkvi, flat: usize) -> Option<(usize, usize)> {
        (flat < Self::library(app).len()).then_some((flat, 0))
    }

    /// The flattened index of the row at `parent` in [`Self::library`].
    /// Default: the same index (no inline expansion).
    fn parent_to_flat(_app: &Nokkvi, parent: usize) -> usize {
        parent
    }

    /// The flattened row count. Default: [`Self::library`]'s length.
    fn flattened_len(app: &Nokkvi) -> usize {
        Self::library(app).len()
    }

    /// The highlighted row (the focus marker a click or a find-and-expand pin
    /// leaves), in terms a reload can find again: its parent row's id and its
    /// place under that parent. Read before a background reload replaces the
    /// rows, whose indices it is relative to.
    fn focused_row(app: &Nokkvi) -> Option<FocusedRow> {
        let list = &Self::page_common(app).slot_list;
        Some(FocusedRow {
            place: Self::row_place(app, list.selected_offset?)?,
            pinned: list.selected_offset_pinned,
        })
    }

    /// The row at flattened index `flat`, by identity (`None` past the end).
    fn row_place(app: &Nokkvi, flat: usize) -> Option<RowPlace> {
        let (parent, below) = Self::flat_to_parent(app, flat)?;
        let id = Self::item_id(Self::library(app).get(parent)?).to_owned();
        Some(RowPlace { id, below })
    }

    /// The flattened index `place` names among the rows now loaded: its
    /// parent's, plus its place under it while an expanded child still sits
    /// there, else the parent's own. `None` when the parent is gone.
    fn flat_index_of(app: &Nokkvi, place: &RowPlace) -> Option<usize> {
        let parent = Self::library(app)
            .iter()
            .position(|item| Self::item_id(item) == place.id)?;
        let parent_flat = Self::parent_to_flat(app, parent);
        let flat = parent_flat + place.below;
        Some(
            if Self::flat_to_parent(app, flat) == Some((parent, place.below)) {
                flat
            } else {
                parent_flat
            },
        )
    }

    /// The row the viewport is on, read before a background reload replaces
    /// the rows. At a flat offset past the parents (an expanded album's
    /// tracks above it) the parent list held no row at that index, so the
    /// reload clamped the viewport to the last parent.
    fn viewport_row(app: &Nokkvi) -> Option<RowPlace> {
        Self::row_place(app, Self::page_common(app).slot_list.viewport_offset)
    }

    /// Put the viewport back on `row` after a background reload, at its
    /// flattened place among the new rows, clamped to them. A parent gone
    /// leaves the viewport where [`Self::apply_viewport_on_load`] put it.
    fn reanchor(app: &mut Nokkvi, row: &RowPlace) {
        let Some(flat) = Self::flat_index_of(app, row) else {
            return;
        };
        let total = Self::flattened_len(app);
        Self::slot_list_mut(app).viewport_offset = flat.min(total.saturating_sub(1));
    }

    /// Put the highlight back on `row` after a background reload. Nothing is
    /// restored when the parent is gone, when an expanded child no longer sits
    /// at the same place under it, or when the row is no longer on screen: an
    /// off-screen highlight would steer Enter and the keys without the user
    /// seeing it.
    fn refocus(app: &mut Nokkvi, row: &FocusedRow) {
        let Some(parent) = Self::library(app)
            .iter()
            .position(|item| Self::item_id(item) == row.place.id)
        else {
            return;
        };
        let flat = Self::parent_to_flat(app, parent) + row.place.below;
        if Self::flat_to_parent(app, flat) != Some((parent, row.place.below)) {
            return;
        }
        let total = Self::flattened_len(app);
        let list = Self::slot_list_mut(app);
        let on_screen =
            (0..list.slot_count).any(|slot| list.slot_to_item_index(slot, total) == Some(flat));
        if !on_screen {
            return;
        }
        if row.pinned {
            list.pin_selected(flat, total);
        } else {
            list.set_selected(flat, total);
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

    fn flat_to_parent(app: &Nokkvi, flat: usize) -> Option<(usize, usize)> {
        let expansion = &app.albums_page.expansion;
        let parents = &app.library.albums;
        let parent = expansion.owning_parent_index(flat, parents, |x| &x.id)?;
        let below = flat.checked_sub(expansion.flat_index_of_parent(parent, parents, |x| &x.id))?;
        Some((parent, below))
    }

    fn parent_to_flat(app: &Nokkvi, parent: usize) -> usize {
        app.albums_page
            .expansion
            .flat_index_of_parent(parent, &app.library.albums, |x| &x.id)
    }

    fn flattened_len(app: &Nokkvi) -> usize {
        app.albums_page.expansion.flattened_len(&app.library.albums)
    }

    fn entity_label() -> &'static str {
        "Albums"
    }

    fn prefetch_artwork_tasks(app: &mut Nokkvi) -> Vec<Task<Message>> {
        let Some(shell) = &app.app_service else {
            return vec![];
        };
        let gate = ArtworkGate::new(&app.artwork);
        let (queued, tasks) = prefetch_album_artwork_tasks(
            &app.albums_page.common.slot_list,
            &app.library.albums,
            &gate,
            shell.albums().clone(),
            album_prefetch_entry,
        );
        drop(gate);
        app.claim_minis(queued);
        tasks
    }

    fn center_large_artwork_task(app: &mut Nokkvi) -> Option<Task<Message>> {
        let album_id = app.centred_album_id()?;
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

    fn flat_to_parent(app: &Nokkvi, flat: usize) -> Option<(usize, usize)> {
        let expansion = &app.artists_page.expansion;
        let parents = &app.library.artists;
        let parent = expansion.owning_parent_index(flat, parents, |x| &x.id)?;
        let below = flat.checked_sub(expansion.flat_index_of_parent(parent, parents, |x| &x.id))?;
        Some((parent, below))
    }

    fn parent_to_flat(app: &Nokkvi, parent: usize) -> usize {
        app.artists_page
            .expansion
            .flat_index_of_parent(parent, &app.library.artists, |x| &x.id)
    }

    fn flattened_len(app: &Nokkvi) -> usize {
        app.artists_page
            .expansion
            .flattened_len(&app.library.artists)
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
        app.app_service.as_ref()?;
        let artist_id = app.centred_artist_id()?;
        Some(app.artist_center_tasks(artist_id))
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
        let mode = app.settings.track_artwork;
        let gate = ArtworkGate::new(&app.artwork);
        let (queued, tasks) = prefetch_song_artwork_tasks(
            &app.songs_page.common.slot_list,
            &app.library.songs,
            &gate,
            shell.albums().clone(),
            |song| song_cover_entry(song, &song.updated_at, mode),
        );
        drop(gate);
        app.claim_minis(queued);
        tasks
    }

    fn center_large_artwork_task(app: &mut Nokkvi) -> Option<Task<Message>> {
        let total = app.library.songs.len();
        let center_idx = app.songs_page.common.get_center_item_index(total)?;
        let key = app
            .library
            .songs
            .get(center_idx)?
            .cover_key(app.settings.track_artwork)?
            .to_owned();
        Some(Task::done(Message::Artwork(ArtworkMessage::LoadLarge(key))))
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
    /// 1. Read `library_page_size` from settings; `load` picks the offset and
    ///    row count ([`PagedLoad`]: a background reload asks for more than a
    ///    page).
    /// 2. **Phase 5A defensive gate** — for a `PagedLoad::Page` past offset 0,
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
        load: PagedLoad,
        msg_ctor: M,
        fetch: F,
    ) -> Task<Message>
    where
        T: LoaderTarget,
        F: FnOnce(AppService, PaginatedFetch) -> Fut + Send + 'static,
        Fut: Future<Output = (Result<Vec<T::Item>, String>, usize)> + Send + 'static,
        M: FnOnce(PagedOutcome<T::Item>) -> Message + Send + 'static,
    {
        let page_size = self.settings.library_page_size.to_usize();
        let viewport_offset = T::page_common(self).slot_list.viewport_offset;
        let (offset, rows) = match load {
            PagedLoad::First => (0, page_size),
            PagedLoad::Reload(rows) => (0, rows),
            PagedLoad::Page(offset) | PagedLoad::ForcedPage(offset) => (offset, page_size),
        };
        // A load that replaces the list takes its place in line, so a newer
        // one (the user's search or sort) outranks it when it lands.
        let replace = matches!(load, PagedLoad::First | PagedLoad::Reload(_))
            .then(|| T::library_mut(self).begin_replace());
        // Phase 5A defensive gate: page-load follow-ups (offset > 0) must
        // pass needs_fetch. Catches duplicate dispatches that race past
        // the upstream needs_fetch check at the action site. Initial
        // loads (offset 0) always proceed — sort/search changes need a
        // fresh page even if the old one is still in flight.
        if matches!(load, PagedLoad::Page(_))
            && offset > 0
            && T::library(self)
                .needs_fetch(viewport_offset, page_size)
                .is_none()
        {
            return Task::none();
        }
        let params =
            PaginatedFetch::from_common(T::page_common(self), T::sort_mode_to_api, offset, rows);
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

        self.shell_task(
            move |shell| fetch(shell, params),
            move |(result, total_count)| {
                msg_ctor(PagedOutcome {
                    result,
                    total_count,
                    replace,
                })
            },
        )
    }

    /// Re-pin the slot-list highlight after a `set_children`-triggering load
    /// (Albums `TracksLoaded`, Artists/Genres `AlbumsLoaded`) when the load
    /// resolves the active find-and-expand chain target.
    ///
    /// The page's `update` clears `selected_offset` when it calls
    /// `set_children`, which would lose the find-chain's intended highlight
    /// position. This helper:
    /// 1. Checks `pending_expand.top_pin` matches the supplied `loaded_id`
    ///    and `expected_kind`.
    /// 2. Looks up the entity index in `library` via `find_position`.
    /// 3. Computes the post-expansion flat-list length via `flattened_len`.
    /// 4. Calls `slot_list.pin_selected(idx, total)` to restore the
    ///    highlight.
    /// 5. Clears `pending_expand.top_pin` so a later, unrelated load doesn't
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
        let Some(pin_state) = &self.pending_expand.top_pin else {
            return;
        };
        if !matches_pin(pin_state, loaded_id) {
            return;
        }
        let Some(idx) = find_position(self, loaded_id) else {
            return;
        };
        pin(self, idx);
        self.pending_expand.top_pin = None;
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

    /// `replace` is the load's place in line (`PagedBuffer::begin_replace`):
    /// an answer a newer list-replacing load has outrun is dropped whole,
    /// rows, count, viewport and error alike, so a background reload of
    /// thousands of rows can't land on top of the search or sort the user
    /// made while it was out. `None` (tests) is never stale.
    pub(crate) fn handle_loaded_with<T: LoaderTarget>(
        &mut self,
        result: Result<Vec<T::Item>, String>,
        total_count: usize,
        background: bool,
        anchor_id: Option<String>,
        replace: Option<ReplaceTicket>,
    ) -> Task<Message> {
        if let Some(ticket) = replace
            && !T::library(self).replace_is_current(ticket)
        {
            debug!(
                " Stale {} load dropped: a newer load replaced the list",
                T::entity_label()
            );
            return Task::none();
        }
        match result {
            Ok(items) => {
                // The header count follows the rows: a failed reload keeps
                // both, instead of "0 genres" above the rows it kept.
                *T::count_mut(self) = total_count;
                debug!(
                    "✅ Loaded {} {} (total: {})",
                    items.len(),
                    T::entity_label(),
                    total_count
                );
                // The highlighted row's identity, read while its indices
                // still name the old rows (background reloads only). So is
                // the viewport's row: the user may have scrolled since the
                // reload was sent, and its send-time anchor would undo that
                // (`anchor_id` stays the fallback for a row since gone).
                let (focused, viewport) = if background {
                    (T::focused_row(self), T::viewport_row(self))
                } else {
                    (None, None)
                };
                T::library_mut(self).set_first_page(items, total_count);
                T::apply_viewport_on_load(self, background, anchor_id.as_deref());
                if let Some(row) = &viewport {
                    T::reanchor(self, row);
                }
                if let Some(row) = &focused {
                    T::refocus(self, row);
                }
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
                if nokkvi_data::types::error::NokkviError::is_unauthorized_str(&e) {
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

    /// The background reload of a paged view: [`reload_rows`] from the top
    /// and the row the viewport is on as the anchor. `None` for a view not
    /// loaded yet (it fetches fresh on its first visit) or scrolled past
    /// [`MAX_RELOAD_ROWS`] (it keeps its rows).
    pub(crate) fn background_reload<T: LoaderTarget>(&self) -> Option<Reload> {
        let library = T::library(self);
        if library.is_empty() {
            return None;
        }
        let slot_list = &T::page_common(self).slot_list;
        let rows = reload_rows(slot_list, self.settings.library_page_size.to_usize())?;
        Some(Reload {
            rows,
            anchor_id: T::viewport_row(self).map(|row| row.id),
        })
    }

    pub(crate) fn handle_page_loaded_with<T: LoaderTarget>(
        &mut self,
        slot: PageSlot,
        result: Result<Vec<T::Item>, String>,
        total_count: usize,
    ) -> Task<Message> {
        // A page lands only where it was asked for. When a reload, sort or
        // search replaced the rows since, or another page landed first, it no
        // longer follows the loaded rows: appending it would repeat rows or
        // mix two orders (the Shift+C find chain's page landing after a
        // background reload), and its failure is nobody's.
        let stale = |entity: &str| {
            debug!(
                "📄 {entity} page for row {} dropped: the list changed since it was asked for",
                slot.offset()
            );
            Task::none()
        };
        match result {
            Ok(new_items) => {
                let count = new_items.len();
                let loaded_before = T::library(self).loaded_count();
                if !T::library_mut(self).append_page_at(slot, new_items, total_count) {
                    return stale(T::entity_label());
                }
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
                if nokkvi_data::types::error::NokkviError::is_unauthorized_str(&e) {
                    T::library_mut(self).set_loading(false);
                    return self.handle_session_expired();
                }
                if !T::library(self).takes_page(slot) {
                    return stale(T::entity_label());
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
