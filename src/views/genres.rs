//! Genres Page Component
//!
//! Self-contained genres view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, container, image},
};
use nokkvi_data::backend::{albums::AlbumUIViewData, genres::GenreUIViewData};

use super::expansion::{ExpansionState, SlotListEntry};
use crate::{
    app_message::Message,
    widgets::{self, SlotListPageState, view_header::SortMode},
};

/// Genres page local state
#[derive(Debug)]
pub struct GenresPage {
    pub common: SlotListPageState,
    /// Inline expansion state (genre → albums)
    pub expansion: ExpansionState<AlbumUIViewData>,
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: GenresColumnVisibility,
}

/// Toggleable genres columns. The genre name (title) is always shown;
/// everything else is user-toggleable through the columns-cog dropdown.
/// The thumbnail flag also drives whether nested child album rows in the
/// genre→album expansion render their artwork column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenresColumn {
    Select,
    Index,
    Thumbnail,
    AlbumCount,
    SongCount,
}

/// User-toggle state for each toggleable genres column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenresColumnVisibility {
    pub select: bool,
    pub index: bool,
    pub thumbnail: bool,
    pub albumcount: bool,
    pub songcount: bool,
}

impl Default for GenresColumnVisibility {
    fn default() -> Self {
        Self {
            select: false,
            index: true,
            thumbnail: true,
            albumcount: true,
            songcount: true,
        }
    }
}

impl GenresColumnVisibility {
    pub fn get(&self, col: GenresColumn) -> bool {
        match col {
            GenresColumn::Select => self.select,
            GenresColumn::Index => self.index,
            GenresColumn::Thumbnail => self.thumbnail,
            GenresColumn::AlbumCount => self.albumcount,
            GenresColumn::SongCount => self.songcount,
        }
    }

    pub fn set(&mut self, col: GenresColumn, value: bool) {
        match col {
            GenresColumn::Select => self.select = value,
            GenresColumn::Index => self.index = value,
            GenresColumn::Thumbnail => self.thumbnail = value,
            GenresColumn::AlbumCount => self.albumcount = value,
            GenresColumn::SongCount => self.songcount = value,
        }
    }
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct GenresViewData<'a> {
    pub genres: &'a [GenreUIViewData],
    pub genre_artwork: &'a HashMap<String, image::Handle>,
    pub genre_collage_artwork: &'a HashMap<String, Vec<image::Handle>>,
    /// Album artwork cache, keyed by album_id. Used by nested child album
    /// rows in the genre→album expansion when `column_visibility.thumbnail`
    /// is enabled.
    pub album_art: &'a HashMap<String, image::Handle>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub total_genre_count: usize,
    pub loading: bool,
    pub stable_viewport: bool,
    /// Whether the column-visibility checkbox dropdown is open (controlled
    /// by `Nokkvi.open_menu`).
    pub column_dropdown_open: bool,
    /// Trigger bounds captured when the dropdown was opened.
    pub column_dropdown_trigger_bounds: Option<iced::Rectangle>,
    /// Borrowed reference to the root open-menu state, so per-row context
    /// menus can resolve their own open/closed status.
    pub open_menu: Option<&'a crate::app_message::OpenMenu>,
}

/// Messages for local genre page interactions
#[derive(Debug, Clone)]
pub enum GenresMessage {
    // Slot list navigation
    SlotListNavigateUp,
    SlotListNavigateDown,
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    SlotListScrollSeek(usize),
    SlotListActivateCenter,
    SlotListClickPlay(usize), // Click non-center to play directly (skip focus)
    /// Click on a row's leading select checkbox — toggles `item_index` in
    /// `selected_indices`. No play/highlight side effects.
    SlotListSelectionToggle(usize),
    /// Click on the tri-state "select all" header — fills selection with
    /// every visible row, or clears if every visible row is already selected.
    SlotListSelectAllToggle,
    AddCenterToQueue, // Add all songs from centered genre to queue (Shift+Q)

    // Mouse click on heart
    ClickToggleStar(usize), // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    // Inline expansion (Shift+Enter on genre)
    ExpandCenter,
    FocusAndExpand(usize), // Clicked 'X albums' — focus that row and expand it
    CollapseExpansion,
    /// Albums loaded for expanded genre (genre_id, albums)
    AlbumsLoaded(String, Vec<AlbumUIViewData>),

    /// Click on a child album row's "X songs" / album-name link, or
    /// Shift+Enter on a centered child album row. Bubbles up as
    /// `GenresAction::NavigateAndExpandAlbum` for cross-view drill-down.
    NavigateAndExpandAlbum(String),

    // View header
    SortModeSelected(widgets::view_header::SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),
    SearchFocused(bool),
    RefreshViewData,
    CenterOnPlaying,

    // Data loading (moved from root Message enum)
    GenresLoaded(Result<Vec<GenreUIViewData>, String>, usize), // result, total_count

    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter), // Navigate to target view and filter
    /// Navigate to Artists and auto-expand the artist with this id (no filter set).
    NavigateAndExpandArtist(String),

    /// Context-menu open/close request — bubbled to root
    /// `Message::SetOpenMenu`. Intercepted in `handle_genres` before the
    /// page's `update` runs.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Artwork column drag handle event — intercepted at root, page never sees it.
    ArtworkColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Toggle a genres column's visibility (currently only Thumbnail).
    ToggleColumnVisible(GenresColumn),
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum GenresAction {
    PlayGenre(String), // genre_id - clear queue and play all songs in genre
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    PlayAlbum(String), // album_id - play child album
    /// Expand genre inline — root should load albums (genre_name, genre_id)
    ExpandGenre(String, String),
    /// Switch to Albums view and prime the named album for inline expansion.
    NavigateAndExpandAlbum(String),
    LoadArtwork(String), // genre_id - load artwork for centered genre on slot list scroll
    PreloadArtwork(usize), // viewport_offset - preload artwork for visible + buffer
    SearchChanged(String), // trigger reload
    SortModeChanged(widgets::view_header::SortMode), // trigger reload
    SortOrderChanged(bool), // trigger reload
    RefreshViewData,     // trigger reload
    ToggleStar(String, &'static str, bool), // (item_id, item_type, starred)
    PlayNextBatch(nokkvi_data::types::batch::BatchPayload),
    AddBatchToPlaylist(nokkvi_data::types::batch::BatchPayload),
    FindSimilar(String, String), // (entity_id, label) - open similar tab
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>), // Open info modal
    ShowAlbumInFolder(String),   // album_id - fetch a song path and open containing folder
    ShowSongInFolder(String),    // song path - open containing folder directly
    CenterOnPlaying,
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter),
    NavigateAndExpandArtist(String), // artist_id - navigate to Artists and auto-expand
    /// Persist a column-visibility toggle change. Root forwards to the
    /// settings service so the new value survives across launches.
    ColumnVisibilityChanged(GenresColumn, bool),
    None,
}

impl super::HasCommonAction for GenresAction {
    fn as_common(&self) -> super::CommonViewAction {
        match self {
            Self::SearchChanged(_) => super::CommonViewAction::SearchChanged,
            Self::SortModeChanged(m) => super::CommonViewAction::SortModeChanged(*m),
            Self::SortOrderChanged(a) => super::CommonViewAction::SortOrderChanged(*a),
            Self::RefreshViewData => super::CommonViewAction::RefreshViewData,
            Self::CenterOnPlaying => super::CommonViewAction::CenterOnPlaying,
            Self::NavigateAndFilter(v, f) => {
                super::CommonViewAction::NavigateAndFilter(*v, f.clone())
            }
            Self::NavigateAndExpandArtist(id) => {
                super::CommonViewAction::NavigateAndExpandArtist(id.clone())
            }
            Self::NavigateAndExpandAlbum(id) => {
                super::CommonViewAction::NavigateAndExpandAlbum(id.clone())
            }
            Self::None => super::CommonViewAction::None,
            _ => super::CommonViewAction::ViewSpecific,
        }
    }
}

impl Default for GenresPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                widgets::view_header::SortMode::Name,
                true, // sort_ascending
            ),
            expansion: ExpansionState::default(),
            column_visibility: GenresColumnVisibility::default(),
        }
    }
}

impl GenresPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert sort mode to API string for server requests.
    /// Thin shim — the unified mapping lives in `views/sort_api.rs`.
    pub fn sort_mode_to_api_string(
        sort_mode: crate::widgets::view_header::SortMode,
    ) -> &'static str {
        super::sort_api::sort_mode_to_api_string(crate::View::Genres, sort_mode)
    }

    /// Resolve the centered item to a LoadArtwork action.
    /// When on a child album, looks up the parent genre's original index.
    fn resolve_artwork_action(&self, genres: &[GenreUIViewData]) -> GenresAction {
        let total = self.expansion.flattened_len(genres);
        if let Some(center_idx) = self.common.get_center_item_index(total) {
            let genre_idx = match self.expansion.get_entry_at(center_idx, genres, |g| &g.id) {
                Some(SlotListEntry::Parent(genre)) => genres.iter().position(|g| g.id == genre.id),
                Some(SlotListEntry::Child(_, parent_id)) => {
                    genres.iter().position(|g| g.id == parent_id)
                }
                None => None,
            };
            if let Some(idx) = genre_idx {
                return GenresAction::LoadArtwork(idx.to_string());
            }
        }
        GenresAction::None
    }

    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: GenresMessage,
        total_items: usize,
        genres: &[GenreUIViewData],
    ) -> (Task<GenresMessage>, GenresAction) {
        // Shift+Enter on a centered child album row: route to the
        // cross-view "navigate to Albums + expand there" path. Mirrors
        // the equivalent block in artists.rs.
        if matches!(message, GenresMessage::ExpandCenter) && self.expansion.is_expanded() {
            let total = self.expansion.flattened_len(genres);
            let center = self
                .common
                .get_center_item_index(total)
                .and_then(|idx| self.expansion.get_entry_at(idx, genres, |g| &g.id));
            if let Some(SlotListEntry::Child(album, _)) = center {
                return (
                    Task::none(),
                    GenresAction::NavigateAndExpandAlbum(album.id.clone()),
                );
            }
        }

        match super::impl_expansion_update!(
            self, message, genres, total_items,
            id_fn: |g| &g.id,
            expand_center: GenresMessage::ExpandCenter => |id: String| {
                let genre_name = genres.iter().find(|g| g.id == id).map(|g| g.name.clone()).unwrap_or_default();
                GenresAction::ExpandGenre(genre_name, id)
            },
            collapse: GenresMessage::CollapseExpansion,
            children_loaded: GenresMessage::AlbumsLoaded,
            sort_selected: GenresMessage::SortModeSelected => GenresAction::SortModeChanged,
            toggle_sort: GenresMessage::ToggleSortOrder => GenresAction::SortOrderChanged,
            search_changed: GenresMessage::SearchQueryChanged => GenresAction::SearchChanged,
            search_focused: GenresMessage::SearchFocused,
            action_none: GenresAction::None,
        ) {
            Ok(result) => result,
            Err(msg) => match msg {
                GenresMessage::FocusAndExpand(offset) => {
                    let len = self.expansion.flattened_len(genres);
                    self.common
                        .handle_slot_click(offset, len, Default::default());
                    if let Some(parent_id) =
                        self.expansion
                            .handle_expand_center(genres, |g| &g.id, &mut self.common)
                    {
                        let genre_name = genres
                            .iter()
                            .find(|g| g.id == parent_id)
                            .map(|g| g.name.clone())
                            .unwrap_or_default();
                        (
                            Task::none(),
                            GenresAction::ExpandGenre(genre_name, parent_id),
                        )
                    } else {
                        (Task::none(), GenresAction::None)
                    }
                }
                GenresMessage::NavigateAndExpandAlbum(album_id) => {
                    (Task::none(), GenresAction::NavigateAndExpandAlbum(album_id))
                }
                GenresMessage::SlotListNavigateUp => {
                    self.expansion.handle_navigate_up(genres, &mut self.common);
                    let action = self.resolve_artwork_action(genres);
                    (Task::none(), action)
                }
                GenresMessage::SlotListNavigateDown => {
                    self.expansion
                        .handle_navigate_down(genres, &mut self.common);
                    let action = self.resolve_artwork_action(genres);
                    (Task::none(), action)
                }
                GenresMessage::SlotListSetOffset(offset, modifiers) => {
                    let len = self.expansion.flattened_len(genres);
                    self.common.handle_slot_click(offset, len, modifiers);
                    let action = self.resolve_artwork_action(genres);
                    (Task::none(), action)
                }
                GenresMessage::SlotListScrollSeek(offset) => {
                    let len = self.expansion.flattened_len(genres);
                    self.common.handle_set_offset(offset, len);
                    (Task::none(), GenresAction::None)
                }
                GenresMessage::SlotListClickPlay(offset) => {
                    let len = self.expansion.flattened_len(genres);
                    self.common.handle_set_offset(offset, len);
                    self.update(GenresMessage::SlotListActivateCenter, total_items, genres)
                }
                GenresMessage::SlotListSelectionToggle(offset) => {
                    let flattened = self.expansion.flattened_len(genres);
                    self.common.handle_selection_toggle(offset, flattened);
                    (Task::none(), GenresAction::None)
                }
                GenresMessage::SlotListSelectAllToggle => {
                    let flattened = self.expansion.flattened_len(genres);
                    self.common.handle_select_all_toggle(flattened);
                    (Task::none(), GenresAction::None)
                }
                GenresMessage::SlotListActivateCenter => {
                    let total = self.expansion.flattened_len(genres);
                    if let Some(center_idx) = self.common.get_center_item_index(total) {
                        self.common.slot_list.flash_center();
                        match self.expansion.get_entry_at(center_idx, genres, |g| &g.id) {
                            Some(SlotListEntry::Child(album, _)) => {
                                (Task::none(), GenresAction::PlayAlbum(album.id.clone()))
                            }
                            Some(SlotListEntry::Parent(genre)) => {
                                (Task::none(), GenresAction::PlayGenre(genre.name.clone()))
                            }
                            None => (Task::none(), GenresAction::None),
                        }
                    } else {
                        (Task::none(), GenresAction::None)
                    }
                }
                GenresMessage::AddCenterToQueue => {
                    use nokkvi_data::types::batch::BatchItem;
                    let total = self.expansion.flattened_len(genres);

                    let target_indices = self.common.get_queue_target_indices(total);

                    if target_indices.is_empty() {
                        return (Task::none(), GenresAction::None);
                    }

                    let payload = super::expansion::build_batch_payload(target_indices, |i| {
                        match self.expansion.get_entry_at(i, genres, |g| &g.id) {
                            Some(SlotListEntry::Parent(genre)) => {
                                Some(BatchItem::Genre(genre.name.clone()))
                            }
                            Some(SlotListEntry::Child(album, _)) => {
                                Some(BatchItem::Album(album.id.clone()))
                            }
                            None => None,
                        }
                    });

                    (Task::none(), GenresAction::AddBatchToQueue(payload))
                }
                GenresMessage::ClickToggleStar(item_index) => {
                    match self.expansion.get_entry_at(item_index, genres, |g| &g.id) {
                        Some(SlotListEntry::Child(album, _)) => (
                            Task::none(),
                            GenresAction::ToggleStar(album.id.clone(), "album", !album.is_starred),
                        ),
                        Some(SlotListEntry::Parent(_genre)) => {
                            // Genres don't have starred state
                            (Task::none(), GenresAction::None)
                        }
                        None => (Task::none(), GenresAction::None),
                    }
                }
                // Data loading messages (handled at root level, no action needed here)
                GenresMessage::GenresLoaded(_, _) => (Task::none(), GenresAction::None),
                // Routed up to root in `handle_genres` before this match runs;
                // arm exists only for exhaustiveness.
                GenresMessage::SetOpenMenu(_) => (Task::none(), GenresAction::None),
                GenresMessage::RefreshViewData => (Task::none(), GenresAction::RefreshViewData),
                GenresMessage::CenterOnPlaying => (Task::none(), GenresAction::CenterOnPlaying),
                GenresMessage::NavigateAndFilter(view, filter) => {
                    (Task::none(), GenresAction::NavigateAndFilter(view, filter))
                }
                GenresMessage::NavigateAndExpandArtist(artist_id) => (
                    Task::none(),
                    GenresAction::NavigateAndExpandArtist(artist_id),
                ),
                GenresMessage::ToggleColumnVisible(col) => {
                    let new_value = !self.column_visibility.get(col);
                    self.column_visibility.set(col, new_value);
                    (
                        Task::none(),
                        GenresAction::ColumnVisibilityChanged(col, new_value),
                    )
                }
                GenresMessage::ContextMenuAction(clicked_idx, entry) => {
                    use nokkvi_data::types::batch::BatchItem;

                    use crate::widgets::context_menu::LibraryContextEntry;

                    match entry {
                        LibraryContextEntry::AddToQueue | LibraryContextEntry::AddToPlaylist => {
                            let target_indices = self.common.get_batch_target_indices(clicked_idx);
                            let payload = super::expansion::build_batch_payload(
                                target_indices,
                                |i| match self.expansion.get_entry_at(i, genres, |g| &g.id) {
                                    Some(SlotListEntry::Parent(genre)) => {
                                        Some(BatchItem::Genre(genre.name.clone()))
                                    }
                                    Some(SlotListEntry::Child(album, _)) => {
                                        Some(BatchItem::Album(album.id.clone()))
                                    }
                                    None => None,
                                },
                            );

                            match entry {
                                LibraryContextEntry::AddToQueue => {
                                    (Task::none(), GenresAction::AddBatchToQueue(payload))
                                }
                                LibraryContextEntry::AddToPlaylist => {
                                    (Task::none(), GenresAction::AddBatchToPlaylist(payload))
                                }
                                _ => unreachable!(),
                            }
                        }
                        // Non-batched actions (apply only to the clicked item)
                        _ => match self.expansion.get_entry_at(clicked_idx, genres, |g| &g.id) {
                            Some(SlotListEntry::Parent(_genre)) => {
                                (Task::none(), GenresAction::None)
                            }
                            Some(SlotListEntry::Child(album, _)) => match entry {
                                LibraryContextEntry::GetInfo => {
                                    use nokkvi_data::types::info_modal::InfoModalItem;
                                    let item = InfoModalItem::Album {
                                        name: album.name.clone(),
                                        album_artist: Some(album.artist.clone()),
                                        release_type: album.release_type.clone(),
                                        genre: album.genre.clone(),
                                        genres: album.genres.clone(),
                                        duration: album.duration,
                                        year: album.year,
                                        song_count: Some(album.song_count),
                                        compilation: album.compilation,
                                        size: album.size,
                                        is_starred: album.is_starred,
                                        rating: album.rating,
                                        play_count: album.play_count,
                                        play_date: album.play_date.clone(),
                                        updated_at: album.updated_at.clone(),
                                        created_at: album.created_at.clone(),
                                        mbz_album_id: album.mbz_album_id.clone(),
                                        comment: album.comment.clone(),
                                        id: album.id.clone(),
                                        tags: album.tags.clone(),
                                        participants: album.participants.clone(),
                                        representative_path: None,
                                    };
                                    (Task::none(), GenresAction::ShowInfo(Box::new(item)))
                                }
                                LibraryContextEntry::ShowInFolder => (
                                    Task::none(),
                                    GenresAction::ShowAlbumInFolder(album.id.clone()),
                                ),
                                LibraryContextEntry::Separator => {
                                    (Task::none(), GenresAction::None)
                                }
                                LibraryContextEntry::FindSimilar => {
                                    let aid = album.artist.clone();
                                    (
                                        Task::none(),
                                        GenresAction::FindSimilar(
                                            aid,
                                            format!("Similar to: {}", album.name),
                                        ),
                                    )
                                }
                                _ => (Task::none(), GenresAction::None),
                            },
                            None => (Task::none(), GenresAction::None),
                        },
                    }
                }
                // Common arms already handled by macro above
                _ => (Task::none(), GenresAction::None),
            },
        }
    }

    /// Build the view
    pub fn view<'a>(&'a self, data: GenresViewData<'a>) -> Element<'a, GenresMessage> {
        use crate::widgets::view_header::SortMode;

        let column_dropdown: Element<'a, GenresMessage> = {
            use crate::widgets::checkbox_dropdown::checkbox_dropdown;
            let items: Vec<(GenresColumn, &'static str, bool)> = vec![
                (
                    GenresColumn::Select,
                    "Select",
                    self.column_visibility.select,
                ),
                (GenresColumn::Index, "Index", self.column_visibility.index),
                (
                    GenresColumn::Thumbnail,
                    "Thumbnail",
                    self.column_visibility.thumbnail,
                ),
                (
                    GenresColumn::AlbumCount,
                    "Album count",
                    self.column_visibility.albumcount,
                ),
                (
                    GenresColumn::SongCount,
                    "Song count",
                    self.column_visibility.songcount,
                ),
            ];
            checkbox_dropdown(
                "assets/icons/columns-3-cog.svg",
                "Show/hide columns",
                items,
                GenresMessage::ToggleColumnVisible,
                |trigger_bounds| match trigger_bounds {
                    Some(b) => GenresMessage::SetOpenMenu(Some(
                        crate::app_message::OpenMenu::CheckboxDropdown {
                            view: crate::View::Genres,
                            trigger_bounds: b,
                        },
                    )),
                    None => GenresMessage::SetOpenMenu(None),
                },
                data.column_dropdown_open,
                data.column_dropdown_trigger_bounds,
            )
            .into()
        };

        let header = widgets::view_header::view_header(
            self.common.current_sort_mode,
            SortMode::GENRE_OPTIONS,
            self.common.sort_ascending,
            &self.common.search_query,
            data.genres.len(),
            data.total_genre_count,
            "genres",
            crate::views::GENRES_SEARCH_ID,
            GenresMessage::SortModeSelected,
            Some(GenresMessage::ToggleSortOrder),
            None, // No shuffle button for genres
            Some(GenresMessage::RefreshViewData),
            Some(GenresMessage::CenterOnPlaying),
            None,                  // on_add
            Some(column_dropdown), // trailing_button
            true,                  // show_search
            GenresMessage::SearchQueryChanged,
        );

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. Tri-state derives from the current
        // selection set against the *flattened* (visible) row count.
        let header = {
            let flattened_len = self.expansion.flattened_len(data.genres);
            crate::widgets::slot_list::compose_header_with_select(
                self.column_visibility.select,
                self.common.select_all_state(flattened_len),
                GenresMessage::SlotListSelectAllToggle,
                header,
            )
        };

        // Create layout config BEFORE empty checks to route empty states through
        // base_slot_list_layout, preserving the widget tree structure and search focus
        use crate::widgets::base_slot_list_layout::BaseSlotListLayoutConfig;
        let layout_config = BaseSlotListLayoutConfig {
            window_width: data.window_width,
            window_height: data.window_height,
            show_artwork_column: true,
        };

        // If loading, show header with loading message
        if data.loading {
            return widgets::base_slot_list_empty_state(header, "Loading...", &layout_config);
        }

        // If no genres match search, show message but keep the header
        if data.genres.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No genres match your search.",
                &layout_config,
            );
        }

        // Configure slot list with genres-specific chrome height (has view header)
        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_select_header, slot_list_view_with_scroll,
        };

        let select_header_visible = self.column_visibility.select;
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            chrome_height_with_select_header(select_header_visible),
        )
        .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let genres = data.genres; // Borrow slice to extend lifetime
        let genre_artwork = data.genre_artwork;
        let genre_collage_artwork = data.genre_collage_artwork;
        let open_menu_for_rows = data.open_menu;

        // Build flattened list (genres + injected albums when expanded)
        let flattened = self.expansion.build_flattened_list(genres, |g| &g.id);
        let center_index = self.common.get_center_item_index(flattened.len());

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            &flattened,
            &config,
            GenresMessage::SlotListNavigateUp,
            GenresMessage::SlotListNavigateDown,
            {
                let total = flattened.len();
                move |f| GenresMessage::SlotListScrollSeek((f * total as f32) as usize)
            },
            |entry, ctx| match entry {
                SlotListEntry::Parent(genre) => {
                    let row = self.render_genre_row(
                        genre,
                        &ctx,
                        genre_artwork,
                        data.stable_viewport,
                        open_menu_for_rows,
                    );
                    crate::widgets::slot_list::wrap_with_select_column(
                        select_header_visible,
                        ctx.is_selected,
                        ctx.item_index,
                        GenresMessage::SlotListSelectionToggle,
                        row,
                    )
                }
                SlotListEntry::Child(album, _parent_genre_id) => {
                    let row = self.render_album_row(
                        album,
                        &ctx,
                        data.album_art,
                        data.stable_viewport,
                        open_menu_for_rows,
                    );
                    crate::widgets::slot_list::wrap_with_select_column(
                        select_header_visible,
                        ctx.is_selected,
                        ctx.item_index,
                        GenresMessage::SlotListSelectionToggle,
                        row,
                    )
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::{collage_artwork_panel, single_artwork_panel};

        // Build artwork column — show parent genre art even when on a child album
        let centered_genre = center_index.and_then(|idx| match flattened.get(idx) {
            Some(SlotListEntry::Parent(genre)) => Some(genre),
            Some(SlotListEntry::Child(_, parent_id)) => genres.iter().find(|g| &g.id == parent_id),
            None => None,
        });
        let genre_id = centered_genre.map(|g| g.id.clone()).unwrap_or_default();

        // Get collage handles for centered genre (borrow, don't clone)
        let collage_handles = genre_collage_artwork.get(&genre_id);

        // Show single full-res when 0-1 albums, collage when 2+ albums
        let album_count = centered_genre.map_or(0, |g| g.album_count);

        let artwork_content = if album_count <= 1 {
            // Show single artwork full-size (use collage[0] if available, else mini)
            let handle = collage_handles
                .and_then(|v| v.first())
                .or_else(|| genre_artwork.get(&genre_id));
            Some(single_artwork_panel::<GenresMessage>(handle))
        } else if let Some(handles) = collage_handles.filter(|v| !v.is_empty()) {
            // Render 3x3 collage grid (2+ albums)
            Some(collage_artwork_panel::<GenresMessage>(handles))
        } else {
            // album_count > 1 but collage NOT loaded yet - show placeholder
            Some(single_artwork_panel::<GenresMessage>(None))
        };

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(GenresMessage::ArtworkColumnDrag),
        )
    }

    /// Render a parent genre row in the slot list
    fn render_genre_row<'a>(
        &self,
        genre: &GenreUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        genre_artwork: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, GenresMessage> {
        use crate::widgets::slot_list::{
            SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column, slot_list_text,
        };

        let is_expanded = self.expansion.is_expanded_parent(&genre.id);
        let style = SlotListSlotStyle::for_slot(
            ctx.is_center,
            is_expanded,
            ctx.is_selected,
            ctx.has_multi_selection,
            ctx.opacity,
            0,
        );

        let m = ctx.metrics;
        let artwork_size = m.artwork_size;
        let title_size = m.title_size;
        let metadata_size = m.metadata_size;
        let index_size = m.metadata_size;

        // Layout: [Index? (5%)] [Artwork?] [Genre Name (45%)] [Album Count? (20%)] [Song Count? (20%)]
        let mut content = iced::widget::Row::new();
        if self.column_visibility.index {
            content = content.push(slot_list_index_column(
                ctx.item_index,
                index_size,
                style,
                ctx.opacity,
            ));
        }
        if self.column_visibility.thumbnail {
            use crate::widgets::slot_list::slot_list_artwork_column;
            content = content.push(slot_list_artwork_column(
                genre_artwork.get(&genre.id),
                artwork_size,
                ctx.is_center,
                false,
                ctx.opacity,
            ));
        }
        content = content.push(
            container(slot_list_text(
                genre.name.clone(),
                title_size,
                style.text_color,
            ))
            .width(Length::FillPortion(45))
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center),
        );
        if self.column_visibility.albumcount {
            use crate::widgets::slot_list::slot_list_metadata_column;
            let album_text = if genre.album_count == 1 {
                "1 album".to_string()
            } else {
                format!("{} albums", genre.album_count)
            };
            let idx = ctx.item_index;
            content = content.push(slot_list_metadata_column(
                album_text,
                Some(GenresMessage::FocusAndExpand(idx)),
                metadata_size,
                style,
                20,
            ));
        }
        if self.column_visibility.songcount {
            use crate::widgets::slot_list::slot_list_metadata_column;
            content = content.push(slot_list_metadata_column(
                format!("{} songs", genre.song_count),
                None,
                metadata_size,
                style,
                20,
            ));
        }
        let content = content
            .spacing(6.0)
            .padding(iced::Padding {
                left: SLOT_LIST_SLOT_PADDING,
                right: 4.0,
                top: 4.0,
                bottom: 4.0,
            })
            .align_y(Alignment::Center)
            .height(Length::Fill);

        let clickable = container(content)
            .style(move |_theme| style.to_container_style())
            .width(Length::Fill);

        let slot_button = button(clickable)
            .on_press(if ctx.modifiers.control() || ctx.modifiers.shift() {
                GenresMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else if ctx.is_center {
                GenresMessage::SlotListActivateCenter
            } else if stable_viewport {
                GenresMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                GenresMessage::SlotListClickPlay(ctx.item_index)
            })
            .style(|_theme, _status| button::Style {
                background: None,
                border: iced::Border::default(),
                ..Default::default()
            })
            .padding(0)
            .width(Length::Fill);

        use crate::widgets::context_menu::{
            context_menu, library_entries, library_entry_view, open_state_for,
        };
        let item_idx = ctx.item_index;
        let cm_id = crate::app_message::ContextMenuId::LibraryRow {
            view: crate::View::Genres,
            item_index: item_idx,
        };
        let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
        context_menu(
            slot_button,
            library_entries(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    GenresMessage::ContextMenuAction(item_idx, e)
                })
            },
            cm_open,
            cm_position,
            move |position| match position {
                Some(p) => {
                    GenresMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: cm_id.clone(),
                        position: p,
                    }))
                }
                None => GenresMessage::SetOpenMenu(None),
            },
        )
        .into()
    }

    /// Render a child album row in the slot list (indented, simpler layout)
    fn render_album_row<'a>(
        &self,
        album: &AlbumUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        album_art: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
    ) -> Element<'a, GenresMessage> {
        let navigate_msg = GenresMessage::NavigateAndExpandAlbum(album.id.clone());
        let album_el = super::expansion::render_child_album_row(
            album,
            ctx,
            album_art.get(&album.id),
            self.column_visibility.thumbnail,
            GenresMessage::SlotListActivateCenter,
            if stable_viewport {
                GenresMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                GenresMessage::SlotListClickPlay(ctx.item_index)
            },
            true, // show artist since genre groups albums from different artists
            Some(GenresMessage::ClickToggleStar(ctx.item_index)),
            Some(navigate_msg.clone()),
            Some(navigate_msg),
            Some(GenresMessage::NavigateAndExpandArtist(
                album.artist_id.clone(),
            )),
            1, // depth 1: child albums under genre
        );

        use crate::widgets::context_menu::{
            context_menu, library_entries_with_folder, library_entry_view, open_state_for,
        };
        let item_idx = ctx.item_index;
        let cm_id = crate::app_message::ContextMenuId::LibraryRow {
            view: crate::View::Genres,
            item_index: item_idx,
        };
        let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
        context_menu(
            album_el,
            library_entries_with_folder(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    GenresMessage::ContextMenuAction(item_idx, e)
                })
            },
            cm_open,
            cm_position,
            move |position| match position {
                Some(p) => {
                    GenresMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: cm_id.clone(),
                        position: p,
                    }))
                }
                None => GenresMessage::SetOpenMenu(None),
            },
        )
        .into()
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

impl super::ViewPage for GenresPage {
    fn common(&self) -> &SlotListPageState {
        &self.common
    }
    fn common_mut(&mut self) -> &mut SlotListPageState {
        &mut self.common
    }

    fn is_expanded(&self) -> bool {
        self.expansion.is_expanded()
    }
    fn collapse_expansion_message(&self) -> Option<Message> {
        Some(Message::Genres(GenresMessage::CollapseExpansion))
    }

    fn search_input_id(&self) -> &'static str {
        super::GENRES_SEARCH_ID
    }

    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        Some(SortMode::GENRE_OPTIONS)
    }
    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Genres(GenresMessage::SortModeSelected(mode)))
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::Genres(GenresMessage::ToggleSortOrder)
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Genres(GenresMessage::AddCenterToQueue))
    }
    fn expand_center_message(&self) -> Option<Message> {
        // ExpandCenter on a 2nd-tier album row routes through `update()`'s
        // pre-check to NavigateAndExpandAlbum (cross-view drill-down);
        // parent rows toggle inline expansion via the macro.
        Some(Message::Genres(GenresMessage::ExpandCenter))
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadGenres)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genres_column_visibility_default_thumbnail_on() {
        let v = GenresColumnVisibility::default();
        assert!(v.thumbnail);
    }

    #[test]
    fn genres_column_visibility_get_set_round_trip() {
        let mut v = GenresColumnVisibility::default();
        assert!(v.get(GenresColumn::Thumbnail));
        v.set(GenresColumn::Thumbnail, false);
        assert!(!v.get(GenresColumn::Thumbnail));
        v.set(GenresColumn::Thumbnail, true);
        assert!(v.get(GenresColumn::Thumbnail));
    }

    #[test]
    fn toggle_column_visible_flips_thumbnail_and_emits_action() {
        let mut page = GenresPage::default();
        let genres: Vec<GenreUIViewData> = Vec::new();

        let (_t, action) = page.update(
            GenresMessage::ToggleColumnVisible(GenresColumn::Thumbnail),
            0,
            &genres,
        );
        assert!(!page.column_visibility.thumbnail);
        assert!(matches!(
            action,
            GenresAction::ColumnVisibilityChanged(GenresColumn::Thumbnail, false)
        ));

        let (_t2, action2) = page.update(
            GenresMessage::ToggleColumnVisible(GenresColumn::Thumbnail),
            0,
            &genres,
        );
        assert!(page.column_visibility.thumbnail);
        assert!(matches!(
            action2,
            GenresAction::ColumnVisibilityChanged(GenresColumn::Thumbnail, true)
        ));
    }
}
