//! Genres Page Component
//!
//! Self-contained genres view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, container, image, row},
};
use nokkvi_data::{
    backend::{albums::AlbumUIViewData, genres::GenreUIViewData},
    utils::scale::calculate_font_size,
};

use super::expansion::{ExpansionState, ThreeTierEntry};
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
    /// Sub-expansion state (album → tracks)
    pub sub_expansion: ExpansionState<nokkvi_data::backend::songs::SongUIViewData>,
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct GenresViewData<'a> {
    pub genres: &'a [GenreUIViewData],
    pub genre_artwork: &'a HashMap<String, image::Handle>,
    pub genre_collage_artwork: &'a HashMap<String, Vec<image::Handle>>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub total_genre_count: usize,
    pub loading: bool,
    pub stable_viewport: bool,
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
    AddCenterToQueue,         // Add all songs from centered genre to queue (Shift+Q)

    // Mouse click on heart
    ClickToggleStar(usize), // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    // Inline expansion — first level (Shift+Enter on genre)
    ExpandCenter,
    FocusAndExpand(usize), // Clicked 'X albums' — focus that row and expand it
    CollapseExpansion,
    /// Albums loaded for expanded genre (genre_id, albums)
    AlbumsLoaded(String, Vec<AlbumUIViewData>),

    // Inline expansion — second level (Shift+Enter on child album)
    ExpandAlbum,
    FocusAndExpandAlbum(usize), // Clicked 'X songs' on child album — focus and expand tracks
    CollapseAlbumExpansion,
    /// Tracks loaded for expanded album (album_id, songs)
    TracksLoaded(String, Vec<nokkvi_data::backend::songs::SongUIViewData>),

    // View header
    SortModeSelected(widgets::view_header::SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),
    SearchFocused(bool),
    RefreshViewData,
    CenterOnPlaying,

    // Data loading (moved from root Message enum)
    GenresLoaded(Result<Vec<GenreUIViewData>, String>, usize), // result, total_count

    NavigateAndSearch(crate::View, String), // Navigate to target view and search
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum GenresAction {
    PlayGenre(String), // genre_id - clear queue and play all songs in genre
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    PlayAlbum(String), // album_id - play child album
    PlayTrack(String), // song_id - play single expanded track
    /// Expand genre inline — root should load albums (genre_name, genre_id)
    ExpandGenre(String, String),
    /// Expand album inline — root should load tracks (album_id)
    ExpandAlbum(String),
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
    NavigateAndSearch(crate::View, String),
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
            Self::NavigateAndSearch(v, q) => {
                super::CommonViewAction::NavigateAndSearch(*v, q.clone())
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
            sub_expansion: ExpansionState::default(),
        }
    }
}

impl GenresPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert sort mode to API string for server requests
    pub fn sort_mode_to_api_string(
        sort_mode: crate::widgets::view_header::SortMode,
    ) -> &'static str {
        match sort_mode {
            crate::widgets::view_header::SortMode::Name => "name",
            crate::widgets::view_header::SortMode::AlbumCount => "albumCount",
            crate::widgets::view_header::SortMode::SongCount => "songCount",
            crate::widgets::view_header::SortMode::Random => "random",
            _ => "name", // Default to name for unsupported types
        }
    }

    /// Resolve the centered item to a LoadArtwork action.
    /// When on a child album or grandchild track, looks up the parent genre's original index.
    fn resolve_artwork_action(&self, genres: &[GenreUIViewData]) -> GenresAction {
        let total = super::expansion::three_tier_flattened_len(
            genres,
            &self.expansion,
            self.sub_expansion.children.len(),
        );
        if let Some(center_idx) = self.common.get_center_item_index(total) {
            let genre_idx = match super::expansion::three_tier_get_entry_at(
                center_idx,
                genres,
                &self.expansion,
                &self.sub_expansion,
                |g| &g.id,
                |a| &a.id,
            ) {
                Some(ThreeTierEntry::Parent(genre)) => genres.iter().position(|g| g.id == genre.id),
                Some(ThreeTierEntry::Child(_, parent_id)) => {
                    genres.iter().position(|g| g.id == parent_id)
                }
                Some(ThreeTierEntry::Grandchild(_, _)) => self
                    .expansion
                    .expanded_id
                    .as_ref()
                    .and_then(|id| genres.iter().position(|g| &g.id == id)),
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
            Ok((task, action)) => {
                if matches!(
                    action,
                    GenresAction::SortModeChanged(_)
                        | GenresAction::SortOrderChanged(_)
                        | GenresAction::SearchChanged(_)
                ) {
                    self.sub_expansion.clear();
                }
                (task, action)
            }
            Err(msg) => match msg {
                GenresMessage::FocusAndExpand(offset) => {
                    let len = super::expansion::three_tier_flattened_len(
                        genres,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common
                        .handle_slot_click(offset, len, Default::default());
                    // Now expand the centered genre
                    if let Some(parent_id) =
                        self.expansion
                            .handle_expand_center(genres, |g| &g.id, &mut self.common)
                    {
                        self.sub_expansion.clear();
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
                GenresMessage::CollapseAlbumExpansion => {
                    let saved = self.sub_expansion.parent_offset;
                    self.sub_expansion.clear();
                    let total =
                        super::expansion::three_tier_flattened_len(genres, &self.expansion, 0);
                    self.common.handle_set_offset(saved, total);
                    (Task::none(), GenresAction::None)
                }
                GenresMessage::FocusAndExpandAlbum(offset) => {
                    let len = super::expansion::three_tier_flattened_len(
                        genres,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common
                        .handle_slot_click(offset, len, Default::default());
                    self.update(GenresMessage::ExpandAlbum, total_items, genres)
                }
                GenresMessage::ExpandAlbum => {
                    let total = super::expansion::three_tier_flattened_len(
                        genres,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    let center_idx = self.common.get_center_item_index(total);
                    let entry = center_idx.and_then(|idx| {
                        super::expansion::three_tier_get_entry_at(
                            idx,
                            genres,
                            &self.expansion,
                            &self.sub_expansion,
                            |g| &g.id,
                            |a| &a.id,
                        )
                    });
                    match entry {
                        Some(ThreeTierEntry::Child(album, _)) => {
                            let aid = album.id.clone();
                            if self.sub_expansion.is_expanded_parent(&aid) {
                                let saved = self.sub_expansion.parent_offset;
                                self.sub_expansion.clear();
                                let total = super::expansion::three_tier_flattened_len(
                                    genres,
                                    &self.expansion,
                                    0,
                                );
                                self.common.handle_set_offset(saved, total);
                                (Task::none(), GenresAction::None)
                            } else {
                                self.sub_expansion.clear();
                                self.sub_expansion.parent_offset =
                                    self.common.slot_list.viewport_offset;
                                (Task::none(), GenresAction::ExpandAlbum(aid))
                            }
                        }
                        Some(ThreeTierEntry::Grandchild(_, _)) => {
                            let saved = self.sub_expansion.parent_offset;
                            self.sub_expansion.clear();
                            let total = super::expansion::three_tier_flattened_len(
                                genres,
                                &self.expansion,
                                0,
                            );
                            self.common.handle_set_offset(saved, total);
                            (Task::none(), GenresAction::None)
                        }
                        _ => (Task::none(), GenresAction::None),
                    }
                }
                GenresMessage::TracksLoaded(album_id, songs) => {
                    self.sub_expansion.set_children(
                        album_id,
                        songs,
                        &self.expansion.children,
                        &mut self.common,
                    );
                    (Task::none(), GenresAction::None)
                }
                GenresMessage::SlotListNavigateUp => {
                    let len = super::expansion::three_tier_flattened_len(
                        genres,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_navigate_up(len);
                    let action = self.resolve_artwork_action(genres);
                    (Task::none(), action)
                }
                GenresMessage::SlotListNavigateDown => {
                    let len = super::expansion::three_tier_flattened_len(
                        genres,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_navigate_down(len);
                    let action = self.resolve_artwork_action(genres);
                    (Task::none(), action)
                }
                GenresMessage::SlotListSetOffset(offset, modifiers) => {
                    let len = super::expansion::three_tier_flattened_len(
                        genres,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_slot_click(offset, len, modifiers);
                    let action = self.resolve_artwork_action(genres);
                    (Task::none(), action)
                }
                GenresMessage::SlotListScrollSeek(offset) => {
                    let len = super::expansion::three_tier_flattened_len(
                        genres,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_set_offset(offset, len);
                    (Task::none(), GenresAction::None)
                }
                GenresMessage::SlotListClickPlay(offset) => {
                    let len = super::expansion::three_tier_flattened_len(
                        genres,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_set_offset(offset, len);
                    self.update(GenresMessage::SlotListActivateCenter, total_items, genres)
                }
                GenresMessage::SlotListActivateCenter => {
                    let total = super::expansion::three_tier_flattened_len(
                        genres,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    if let Some(center_idx) = self.common.get_center_item_index(total) {
                        self.common.slot_list.flash_center();
                        match super::expansion::three_tier_get_entry_at(
                            center_idx,
                            genres,
                            &self.expansion,
                            &self.sub_expansion,
                            |g| &g.id,
                            |a| &a.id,
                        ) {
                            Some(ThreeTierEntry::Grandchild(song, _)) => {
                                (Task::none(), GenresAction::PlayTrack(song.id.clone()))
                            }
                            Some(ThreeTierEntry::Child(album, _)) => {
                                (Task::none(), GenresAction::PlayAlbum(album.id.clone()))
                            }
                            Some(ThreeTierEntry::Parent(genre)) => {
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
                    let total = super::expansion::three_tier_flattened_len(
                        genres,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );

                    let target_indices = self.common.get_queue_target_indices(total);

                    if target_indices.is_empty() {
                        return (Task::none(), GenresAction::None);
                    }

                    let payload = super::expansion::build_batch_payload(target_indices, |i| {
                        match super::expansion::three_tier_get_entry_at(
                            i,
                            genres,
                            &self.expansion,
                            &self.sub_expansion,
                            |g| &g.id,
                            |a| &a.id,
                        ) {
                            Some(ThreeTierEntry::Parent(genre)) => {
                                Some(BatchItem::Genre(genre.name.clone()))
                            }
                            Some(ThreeTierEntry::Child(album, _)) => {
                                Some(BatchItem::Album(album.id.clone()))
                            }
                            Some(ThreeTierEntry::Grandchild(song, _)) => {
                                let item: nokkvi_data::types::song::Song = song.clone().into();
                                Some(BatchItem::Song(Box::new(item)))
                            }
                            None => None,
                        }
                    });

                    (Task::none(), GenresAction::AddBatchToQueue(payload))
                }
                GenresMessage::ClickToggleStar(item_index) => {
                    match super::expansion::three_tier_get_entry_at(
                        item_index,
                        genres,
                        &self.expansion,
                        &self.sub_expansion,
                        |g| &g.id,
                        |a| &a.id,
                    ) {
                        Some(ThreeTierEntry::Grandchild(song, _)) => (
                            Task::none(),
                            GenresAction::ToggleStar(song.id.clone(), "song", !song.is_starred),
                        ),
                        Some(ThreeTierEntry::Child(album, _)) => (
                            Task::none(),
                            GenresAction::ToggleStar(album.id.clone(), "album", !album.is_starred),
                        ),
                        Some(ThreeTierEntry::Parent(_genre)) => {
                            // Genres don't have starred state
                            (Task::none(), GenresAction::None)
                        }
                        None => (Task::none(), GenresAction::None),
                    }
                }
                // Data loading messages (handled at root level, no action needed here)
                GenresMessage::GenresLoaded(_, _) => (Task::none(), GenresAction::None),
                GenresMessage::RefreshViewData => (Task::none(), GenresAction::RefreshViewData),
                GenresMessage::CenterOnPlaying => (Task::none(), GenresAction::CenterOnPlaying),
                GenresMessage::NavigateAndSearch(view, query) => {
                    (Task::none(), GenresAction::NavigateAndSearch(view, query))
                }
                GenresMessage::ContextMenuAction(clicked_idx, entry) => {
                    use nokkvi_data::types::batch::BatchItem;

                    use crate::widgets::context_menu::LibraryContextEntry;

                    match entry {
                        LibraryContextEntry::AddToQueue | LibraryContextEntry::AddToPlaylist => {
                            let target_indices = self.common.get_batch_target_indices(clicked_idx);
                            let payload =
                                super::expansion::build_batch_payload(target_indices, |i| {
                                    match super::expansion::three_tier_get_entry_at(
                                        i,
                                        genres,
                                        &self.expansion,
                                        &self.sub_expansion,
                                        |g| &g.id,
                                        |a| &a.id,
                                    ) {
                                        Some(ThreeTierEntry::Parent(genre)) => {
                                            Some(BatchItem::Genre(genre.name.clone()))
                                        }
                                        Some(ThreeTierEntry::Child(album, _)) => {
                                            Some(BatchItem::Album(album.id.clone()))
                                        }
                                        Some(ThreeTierEntry::Grandchild(song, _)) => {
                                            let item: nokkvi_data::types::song::Song =
                                                song.clone().into();
                                            Some(BatchItem::Song(Box::new(item)))
                                        }
                                        None => None,
                                    }
                                });

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
                        _ => {
                            match super::expansion::three_tier_get_entry_at(
                                clicked_idx,
                                genres,
                                &self.expansion,
                                &self.sub_expansion,
                                |g| &g.id,
                                |a| &a.id,
                            ) {
                                Some(ThreeTierEntry::Parent(_genre)) => {
                                    (Task::none(), GenresAction::None)
                                }
                                Some(ThreeTierEntry::Child(album, _)) => match entry {
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
                                            representative_path: self
                                                .sub_expansion
                                                .children
                                                .first()
                                                .map(|s| s.path.clone()),
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
                                Some(ThreeTierEntry::Grandchild(song, _)) => match entry {
                                    LibraryContextEntry::GetInfo => {
                                        use nokkvi_data::types::info_modal::InfoModalItem;
                                        let item = InfoModalItem::from_song_view_data(song);
                                        (Task::none(), GenresAction::ShowInfo(Box::new(item)))
                                    }
                                    LibraryContextEntry::ShowInFolder => (
                                        Task::none(),
                                        GenresAction::ShowSongInFolder(song.path.clone()),
                                    ),
                                    LibraryContextEntry::Separator => {
                                        (Task::none(), GenresAction::None)
                                    }
                                    LibraryContextEntry::FindSimilar => (
                                        Task::none(),
                                        GenresAction::FindSimilar(
                                            song.id.clone(),
                                            format!("Similar to: {}", song.title),
                                        ),
                                    ),
                                    _ => (Task::none(), GenresAction::None),
                                },
                                None => (Task::none(), GenresAction::None),
                            }
                        }
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
            true, // show_search
            GenresMessage::SearchQueryChanged,
        );

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
            SlotListConfig, chrome_height_with_header, slot_list_view_with_scroll,
        };

        let config =
            SlotListConfig::with_dynamic_slots(data.window_height, chrome_height_with_header())
                .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let genres = data.genres; // Borrow slice to extend lifetime
        let genre_artwork = data.genre_artwork;
        let genre_collage_artwork = data.genre_collage_artwork;

        // Build flattened list (genres + injected albums + injected tracks when expanded)
        let flattened = super::expansion::build_three_tier_list(
            genres,
            &self.expansion,
            &self.sub_expansion,
            |g| &g.id,
            |a| &a.id,
        );
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
                ThreeTierEntry::Parent(genre) => {
                    self.render_genre_row(genre, &ctx, genre_artwork, data.stable_viewport)
                }
                ThreeTierEntry::Child(album, _parent_genre_id) => {
                    self.render_album_row(album, &ctx, data.stable_viewport)
                }
                ThreeTierEntry::Grandchild(song, _album_id) => {
                    let track_el = super::expansion::render_child_track_row(
                        song,
                        &ctx,
                        GenresMessage::SlotListActivateCenter,
                        if data.stable_viewport {
                            GenresMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
                        } else {
                            GenresMessage::SlotListClickPlay(ctx.item_index)
                        },
                        Some(GenresMessage::ClickToggleStar(ctx.item_index)),
                        Some(GenresMessage::NavigateAndSearch(
                            crate::View::Artists,
                            song.artist.clone(),
                        )),
                        2, // depth 2: grandchild tracks (genre → album → track)
                    );
                    use crate::widgets::context_menu::{
                        context_menu, library_entry_view, song_entries_with_folder,
                    };
                    let item_idx = ctx.item_index;
                    context_menu(
                        track_el,
                        song_entries_with_folder(),
                        move |entry, length| {
                            library_entry_view(entry, length, |e| {
                                GenresMessage::ContextMenuAction(item_idx, e)
                            })
                        },
                    )
                    .into()
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::{
            base_slot_list_layout, collage_artwork_panel, single_artwork_panel,
        };

        // Build artwork column — show parent genre art even when on a child album
        let centered_genre = center_index.and_then(|idx| match flattened.get(idx) {
            Some(ThreeTierEntry::Parent(genre)) => Some(genre),
            Some(ThreeTierEntry::Child(_, parent_id)) => genres.iter().find(|g| &g.id == parent_id),
            Some(ThreeTierEntry::Grandchild(_, _)) => self
                .expansion
                .expanded_id
                .as_ref()
                .and_then(|id| genres.iter().find(|g| &g.id == id)),
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

        base_slot_list_layout(&layout_config, header, slot_list_content, artwork_content)
    }

    /// Render a parent genre row in the slot list
    fn render_genre_row<'a>(
        &self,
        genre: &GenreUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        genre_artwork: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
    ) -> Element<'a, GenresMessage> {
        use crate::widgets::slot_list::{
            SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column,
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

        let base_artwork_size = (ctx.row_height - 16.0).max(32.0);
        let artwork_size = base_artwork_size * ctx.scale_factor;
        let title_size =
            calculate_font_size(14.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
        let metadata_size =
            calculate_font_size(12.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
        let index_size =
            calculate_font_size(12.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;

        // Layout: [Index (5%)] [Artwork] [Genre Name (45%)] [Album Count (20%)] [Song Count (20%)]
        let content = row![
            slot_list_index_column(ctx.item_index, index_size, style, ctx.opacity),
            {
                use crate::widgets::slot_list::slot_list_artwork_column;
                slot_list_artwork_column(
                    genre_artwork.get(&genre.id),
                    artwork_size,
                    ctx.is_center,
                    false,
                    ctx.opacity,
                )
            },
            container({
                let click_title = Some(GenresMessage::ContextMenuAction(
                    ctx.item_index,
                    crate::widgets::context_menu::LibraryContextEntry::GetInfo,
                ));
                let link_color = if ctx.is_center {
                    style.text_color
                } else {
                    crate::theme::accent_bright()
                };
                crate::widgets::link_text::LinkText::new(genre.name.clone())
                    .size(title_size)
                    .color(style.text_color)
                    .hover_color(link_color)
                    .on_press(click_title)
            })
            .width(Length::FillPortion(45))
            .height(Length::Fill)
            .clip(true)
            .align_y(Alignment::Center),
            {
                use crate::widgets::slot_list::slot_list_metadata_column;
                let album_text = if genre.album_count == 1 {
                    "1 album".to_string()
                } else {
                    format!("{} albums", genre.album_count)
                };
                let idx = ctx.item_index;
                slot_list_metadata_column(
                    album_text,
                    Some(GenresMessage::FocusAndExpand(idx)),
                    metadata_size,
                    style,
                    20,
                )
            },
            {
                use crate::widgets::slot_list::slot_list_metadata_column;
                slot_list_metadata_column(
                    format!("{} songs", genre.song_count),
                    None,
                    metadata_size,
                    style,
                    20,
                )
            },
        ]
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

        use crate::widgets::context_menu::{context_menu, library_entries, library_entry_view};
        let item_idx = ctx.item_index;
        context_menu(slot_button, library_entries(), move |entry, length| {
            library_entry_view(entry, length, |e| {
                GenresMessage::ContextMenuAction(item_idx, e)
            })
        })
        .into()
    }

    /// Render a child album row in the slot list (indented, simpler layout)
    fn render_album_row<'a>(
        &self,
        album: &AlbumUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        stable_viewport: bool,
    ) -> Element<'a, GenresMessage> {
        let album_el = super::expansion::render_child_album_row(
            album,
            ctx,
            GenresMessage::SlotListActivateCenter,
            if stable_viewport {
                GenresMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                GenresMessage::SlotListClickPlay(ctx.item_index)
            },
            true, // show artist since genre groups albums from different artists
            Some(GenresMessage::ClickToggleStar(ctx.item_index)),
            Some(GenresMessage::FocusAndExpandAlbum(ctx.item_index)),
            Some(GenresMessage::NavigateAndSearch(
                crate::View::Albums,
                album.name.clone(),
            )),
            Some(GenresMessage::NavigateAndSearch(
                crate::View::Artists,
                album.artist.clone(),
            )),
            1, // depth 1: child albums under genre
        );

        use crate::widgets::context_menu::{
            context_menu, library_entries_with_folder, library_entry_view,
        };
        let item_idx = ctx.item_index;
        context_menu(
            album_el,
            library_entries_with_folder(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    GenresMessage::ContextMenuAction(item_idx, e)
                })
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
        self.expansion.is_expanded() || self.sub_expansion.is_expanded()
    }
    fn collapse_expansion_message(&self) -> Option<Message> {
        if self.sub_expansion.is_expanded() {
            Some(Message::Genres(GenresMessage::CollapseAlbumExpansion))
        } else {
            Some(Message::Genres(GenresMessage::CollapseExpansion))
        }
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
        if self.expansion.is_expanded() {
            Some(Message::Genres(GenresMessage::ExpandAlbum))
        } else {
            Some(Message::Genres(GenresMessage::ExpandCenter))
        }
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadGenres)
    }
}
