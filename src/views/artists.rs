//! Artists Page Component
//!
//! Self-contained artists view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.
//! Supports inline album expansion (Shift+Enter) using flattened SlotListEntry list.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, container, image, row},
};
use nokkvi_data::{
    backend::{albums::AlbumUIViewData, artists::ArtistUIViewData},
    utils::scale::calculate_font_size,
};

use super::expansion::{ExpansionState, ThreeTierEntry};
use crate::{
    app_message::Message,
    widgets::{self, SlotListPageState, view_header::SortMode},
};

/// Artists page local state
#[derive(Debug)]
pub struct ArtistsPage {
    pub common: SlotListPageState,
    /// Inline expansion state (artist → albums)
    pub expansion: ExpansionState<AlbumUIViewData>,
    /// Sub-expansion state (album → tracks)
    pub sub_expansion: ExpansionState<nokkvi_data::backend::songs::SongUIViewData>,
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct ArtistsViewData<'a> {
    pub artists: &'a [ArtistUIViewData],
    pub artist_art: &'a HashMap<String, image::Handle>,
    pub large_artwork: &'a HashMap<String, image::Handle>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub total_artist_count: usize,
    pub loading: bool,
    pub stable_viewport: bool,
}

/// Messages for local artist page interactions
#[derive(Debug, Clone)]
pub enum ArtistsMessage {
    // Slot list navigation
    SlotListNavigateUp,
    SlotListNavigateDown,
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    SlotListScrollSeek(usize),
    SlotListActivateCenter,
    SlotListClickPlay(usize), // Click non-center to play directly (skip focus)
    AddCenterToQueue,         // Add all songs from centered artist to queue (Shift+Q)
    ToggleCenterStar,         // Toggle star on centered artist (L key)

    // Mouse click on star/heart (item_index, value)
    ClickSetRating(usize, usize), // (item_index, rating 1-5)
    ClickToggleStar(usize),       // item_index

    // Context menu
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),

    // Inline expansion — first level (Shift+Enter on artist)
    ExpandCenter,
    CollapseExpansion,
    /// Albums loaded for expanded artist (artist_id, albums)
    AlbumsLoaded(String, Vec<AlbumUIViewData>),

    // Inline expansion — second level (Shift+Enter on child album)
    ExpandAlbum,
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
    ArtistsLoaded(Result<Vec<ArtistUIViewData>, String>, usize), // result, total_count
    ArtistsPageLoaded(Result<Vec<ArtistUIViewData>, String>, usize), // result, total_count (subsequent page)
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum ArtistsAction {
    PlayArtist(String), // artist_id - clear queue and play all songs
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    PlayAlbum(String),    // album_id - play child album
    PlayTrack(String),    // song_id - play single expanded track
    StarArtist(String),   // artist_id - star the artist
    UnstarArtist(String), // artist_id - unstar the artist
    /// Set absolute rating on item (item_id, item_type, rating)
    SetRating(String, &'static str, usize),
    /// Star/unstar item by click (item_id, item_type, new_starred)
    ToggleStar(String, &'static str, bool),
    /// Expand artist inline — root should load albums (artist_id)
    ExpandArtist(String),
    /// Expand album inline — root should load tracks (album_id)
    ExpandAlbum(String),
    LoadPage(usize),       // offset - trigger fetch of next page
    SearchChanged(String), // trigger reload
    SortModeChanged(widgets::view_header::SortMode), // trigger reload
    SortOrderChanged(bool), // trigger reload
    RefreshViewData,       // trigger reload
    PlayNextBatch(nokkvi_data::types::batch::BatchPayload), // artist_id or album_id - insert after currently playing
    AddBatchToPlaylist(nokkvi_data::types::batch::BatchPayload),
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>), // Open info modal
    ShowAlbumInFolder(String), // album_id - fetch a song path and open containing folder
    ShowSongInFolder(String),  // song path - open containing folder directly
    CenterOnPlaying,
    None,
}

impl super::HasCommonAction for ArtistsAction {
    fn as_common(&self) -> super::CommonViewAction {
        match self {
            Self::SearchChanged(_) => super::CommonViewAction::SearchChanged,
            Self::SortModeChanged(m) => super::CommonViewAction::SortModeChanged(*m),
            Self::SortOrderChanged(a) => super::CommonViewAction::SortOrderChanged(*a),
            Self::RefreshViewData => super::CommonViewAction::RefreshViewData,
            Self::CenterOnPlaying => super::CommonViewAction::CenterOnPlaying,
            Self::None => super::CommonViewAction::None,
            _ => super::CommonViewAction::ViewSpecific,
        }
    }
}

impl Default for ArtistsPage {
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

impl ArtistsPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert sort mode to API string for server requests
    pub fn sort_mode_to_api_string(
        sort_mode: crate::widgets::view_header::SortMode,
    ) -> &'static str {
        use crate::widgets::view_header::SortMode;
        match sort_mode {
            SortMode::Name => "name",
            SortMode::Favorited => "favorited",
            SortMode::AlbumCount => "albumCount",
            SortMode::SongCount => "songCount",
            SortMode::Random => "random",
            SortMode::Rating => "name", // load all, sort client-side
            _ => "random",              // Default to random for artists
        }
    }

    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: ArtistsMessage,
        total_items: usize,
        artists: &[ArtistUIViewData],
    ) -> (Task<ArtistsMessage>, ArtistsAction) {
        match super::impl_expansion_update!(
            self, message, artists, total_items,
            id_fn: |a| &a.id,
            expand_center: ArtistsMessage::ExpandCenter => ArtistsAction::ExpandArtist,
            collapse: ArtistsMessage::CollapseExpansion,
            children_loaded: ArtistsMessage::AlbumsLoaded,
            sort_selected: ArtistsMessage::SortModeSelected => ArtistsAction::SortModeChanged,
            toggle_sort: ArtistsMessage::ToggleSortOrder => ArtistsAction::SortOrderChanged,
            search_changed: ArtistsMessage::SearchQueryChanged => ArtistsAction::SearchChanged,
            search_focused: ArtistsMessage::SearchFocused,
            action_none: ArtistsAction::None,
        ) {
            Ok((task, action)) => {
                // Clear sub_expansion when the outer expansion is collapsed or reloaded
                if matches!(
                    action,
                    ArtistsAction::SortModeChanged(_)
                        | ArtistsAction::SortOrderChanged(_)
                        | ArtistsAction::SearchChanged(_)
                ) {
                    self.sub_expansion.clear();
                }
                (task, action)
            }
            Err(msg) => match msg {
                // CollapseExpansion handled by macro — clear sub_expansion too
                ArtistsMessage::CollapseAlbumExpansion => {
                    // Restore position to where user was when album was expanded
                    let saved = self.sub_expansion.parent_offset;
                    self.sub_expansion.clear();
                    let total =
                        super::expansion::three_tier_flattened_len(artists, &self.expansion, 0);
                    self.common.handle_set_offset(saved, total);
                    (Task::none(), ArtistsAction::None)
                }
                ArtistsMessage::ExpandAlbum => {
                    // Shift+Enter on a child album row — expand its tracks
                    let total = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    let center_idx = self.common.get_center_item_index(total);
                    let action = center_idx.and_then(|idx| {
                        super::expansion::three_tier_get_entry_at(
                            idx,
                            artists,
                            &self.expansion,
                            &self.sub_expansion,
                            |a| &a.id,
                            |a| &a.id,
                        )
                    });
                    match action {
                        Some(ThreeTierEntry::Child(album, _)) => {
                            // Toggle: if already expanded, collapse
                            let aid = album.id.clone();
                            if self.sub_expansion.is_expanded_parent(&aid) {
                                let saved = self.sub_expansion.parent_offset;
                                self.sub_expansion.clear();
                                let total = super::expansion::three_tier_flattened_len(
                                    artists,
                                    &self.expansion,
                                    0,
                                );
                                self.common.handle_set_offset(saved, total);
                                (Task::none(), ArtistsAction::None)
                            } else {
                                // Collapse any existing sub-expansion, start new one
                                self.sub_expansion.clear();
                                self.sub_expansion.parent_offset =
                                    self.common.slot_list.viewport_offset;
                                (Task::none(), ArtistsAction::ExpandAlbum(aid))
                            }
                        }
                        Some(ThreeTierEntry::Grandchild(_, _)) => {
                            // On a grandchild — collapse the album sub-expansion
                            let saved = self.sub_expansion.parent_offset;
                            self.sub_expansion.clear();
                            let total = super::expansion::three_tier_flattened_len(
                                artists,
                                &self.expansion,
                                0,
                            );
                            self.common.handle_set_offset(saved, total);
                            (Task::none(), ArtistsAction::None)
                        }
                        _ => (Task::none(), ArtistsAction::None), // On parent or nothing
                    }
                }
                ArtistsMessage::TracksLoaded(album_id, songs) => {
                    self.sub_expansion.set_children(
                        album_id,
                        songs,
                        &self.expansion.children,
                        &mut self.common,
                    );
                    (Task::none(), ArtistsAction::None)
                }
                ArtistsMessage::SlotListNavigateUp => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_navigate_up(len);
                    (Task::none(), ArtistsAction::None)
                }
                ArtistsMessage::SlotListNavigateDown => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_navigate_down(len);
                    (Task::none(), ArtistsAction::None)
                }
                ArtistsMessage::SlotListSetOffset(offset, modifiers) => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_slot_click(offset, len, modifiers);
                    (Task::none(), ArtistsAction::None)
                }
                ArtistsMessage::SlotListScrollSeek(offset) => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_set_offset(offset, len);
                    (Task::none(), ArtistsAction::None)
                }
                ArtistsMessage::SlotListClickPlay(offset) => {
                    let len = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    self.common.handle_set_offset(offset, len);
                    self.update(ArtistsMessage::SlotListActivateCenter, total_items, artists)
                }
                ArtistsMessage::SlotListActivateCenter => {
                    let total = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );
                    if let Some(center_idx) = self.common.get_center_item_index(total) {
                        self.common.slot_list.flash_center();
                        match super::expansion::three_tier_get_entry_at(
                            center_idx,
                            artists,
                            &self.expansion,
                            &self.sub_expansion,
                            |a| &a.id,
                            |a| &a.id,
                        ) {
                            Some(ThreeTierEntry::Grandchild(song, _)) => {
                                (Task::none(), ArtistsAction::PlayTrack(song.id.clone()))
                            }
                            Some(ThreeTierEntry::Child(album, _)) => {
                                (Task::none(), ArtistsAction::PlayAlbum(album.id.clone()))
                            }
                            Some(ThreeTierEntry::Parent(_)) => (
                                Task::none(),
                                ArtistsAction::PlayArtist(center_idx.to_string()),
                            ),
                            None => (Task::none(), ArtistsAction::None),
                        }
                    } else {
                        (Task::none(), ArtistsAction::None)
                    }
                }
                ArtistsMessage::AddCenterToQueue => {
                    use nokkvi_data::types::batch::BatchItem;
                    let total = super::expansion::three_tier_flattened_len(
                        artists,
                        &self.expansion,
                        self.sub_expansion.children.len(),
                    );

                    let target_indices = self.common.get_queue_target_indices(total);

                    if target_indices.is_empty() {
                        return (Task::none(), ArtistsAction::None);
                    }

                    let payload = super::expansion::build_batch_payload(target_indices, |i| {
                        match super::expansion::three_tier_get_entry_at(
                            i,
                            artists,
                            &self.expansion,
                            &self.sub_expansion,
                            |a| &a.id,
                            |a| &a.id,
                        ) {
                            Some(ThreeTierEntry::Parent(artist)) => {
                                Some(BatchItem::Artist(artist.id.clone()))
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

                    (Task::none(), ArtistsAction::AddBatchToQueue(payload))
                }
                ArtistsMessage::ToggleCenterStar => {
                    if let Some(center_idx) = self.common.get_center_item_index(total_items) {
                        if let Some(artist) = artists.get(center_idx) {
                            if artist.is_starred {
                                (Task::none(), ArtistsAction::UnstarArtist(artist.id.clone()))
                            } else {
                                (Task::none(), ArtistsAction::StarArtist(artist.id.clone()))
                            }
                        } else {
                            (Task::none(), ArtistsAction::None)
                        }
                    } else {
                        (Task::none(), ArtistsAction::None)
                    }
                }
                // Data loading messages (handled at root level, no action needed here)
                ArtistsMessage::ArtistsLoaded(_, _) => (Task::none(), ArtistsAction::None),
                ArtistsMessage::ArtistsPageLoaded(_, _) => (Task::none(), ArtistsAction::None),
                ArtistsMessage::RefreshViewData => (Task::none(), ArtistsAction::RefreshViewData),
                ArtistsMessage::CenterOnPlaying => (Task::none(), ArtistsAction::CenterOnPlaying),
                ArtistsMessage::ClickSetRating(item_index, rating) => {
                    use nokkvi_data::utils::formatters::compute_rating_toggle;
                    match super::expansion::three_tier_get_entry_at(
                        item_index,
                        artists,
                        &self.expansion,
                        &self.sub_expansion,
                        |a| &a.id,
                        |a| &a.id,
                    ) {
                        Some(ThreeTierEntry::Grandchild(song, _)) => {
                            let current = song.rating.unwrap_or(0) as usize;
                            let new_rating = compute_rating_toggle(current, rating);
                            (
                                Task::none(),
                                ArtistsAction::SetRating(song.id.clone(), "song", new_rating),
                            )
                        }
                        Some(ThreeTierEntry::Child(album, _)) => {
                            let current = album.rating.unwrap_or(0) as usize;
                            let new_rating = compute_rating_toggle(current, rating);
                            (
                                Task::none(),
                                ArtistsAction::SetRating(album.id.clone(), "album", new_rating),
                            )
                        }
                        Some(ThreeTierEntry::Parent(artist)) => {
                            let current = artist.rating.unwrap_or(0) as usize;
                            let new_rating = compute_rating_toggle(current, rating);
                            (
                                Task::none(),
                                ArtistsAction::SetRating(artist.id.clone(), "artist", new_rating),
                            )
                        }
                        None => (Task::none(), ArtistsAction::None),
                    }
                }
                ArtistsMessage::ClickToggleStar(item_index) => {
                    match super::expansion::three_tier_get_entry_at(
                        item_index,
                        artists,
                        &self.expansion,
                        &self.sub_expansion,
                        |a| &a.id,
                        |a| &a.id,
                    ) {
                        Some(ThreeTierEntry::Grandchild(song, _)) => (
                            Task::none(),
                            ArtistsAction::ToggleStar(song.id.clone(), "song", !song.is_starred),
                        ),
                        Some(ThreeTierEntry::Child(album, _)) => (
                            Task::none(),
                            ArtistsAction::ToggleStar(album.id.clone(), "album", !album.is_starred),
                        ),
                        Some(ThreeTierEntry::Parent(artist)) => (
                            Task::none(),
                            ArtistsAction::ToggleStar(
                                artist.id.clone(),
                                "artist",
                                !artist.is_starred,
                            ),
                        ),
                        None => (Task::none(), ArtistsAction::None),
                    }
                }
                ArtistsMessage::ContextMenuAction(clicked_idx, entry) => {
                    use nokkvi_data::types::batch::BatchItem;

                    use crate::widgets::context_menu::LibraryContextEntry;

                    match entry {
                        LibraryContextEntry::AddToQueue | LibraryContextEntry::AddToPlaylist => {
                            let target_indices = self.common.get_batch_target_indices(clicked_idx);
                            let payload =
                                super::expansion::build_batch_payload(target_indices, |i| {
                                    match super::expansion::three_tier_get_entry_at(
                                        i,
                                        artists,
                                        &self.expansion,
                                        &self.sub_expansion,
                                        |a| &a.id,
                                        |a| &a.id,
                                    ) {
                                        Some(ThreeTierEntry::Parent(artist)) => {
                                            Some(BatchItem::Artist(artist.id.clone()))
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
                                    (Task::none(), ArtistsAction::AddBatchToQueue(payload))
                                }
                                LibraryContextEntry::AddToPlaylist => {
                                    (Task::none(), ArtistsAction::AddBatchToPlaylist(payload))
                                }
                                _ => unreachable!(),
                            }
                        }
                        // Non-batched actions (apply only to the clicked item)
                        _ => {
                            match super::expansion::three_tier_get_entry_at(
                                clicked_idx,
                                artists,
                                &self.expansion,
                                &self.sub_expansion,
                                |a| &a.id,
                                |a| &a.id,
                            ) {
                                Some(ThreeTierEntry::Parent(artist)) => match entry {
                                    LibraryContextEntry::GetInfo => {
                                        use nokkvi_data::types::info_modal::InfoModalItem;
                                        let item = InfoModalItem::Artist {
                                            name: artist.name.clone(),
                                            song_count: Some(artist.song_count),
                                            album_count: Some(artist.album_count),
                                            is_starred: artist.is_starred,
                                            rating: artist.rating,
                                            play_count: artist.play_count,
                                            play_date: artist.play_date.clone(),
                                            size: artist.size,
                                            mbz_artist_id: artist.mbz_artist_id.clone(),
                                            biography: artist.biography.clone(),
                                            external_url: artist.external_url.clone(),
                                            id: artist.id.clone(),
                                        };
                                        (Task::none(), ArtistsAction::ShowInfo(Box::new(item)))
                                    }
                                    LibraryContextEntry::ShowInFolder
                                    | LibraryContextEntry::Separator => {
                                        (Task::none(), ArtistsAction::None)
                                    }
                                    _ => (Task::none(), ArtistsAction::None),
                                },
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
                                        (Task::none(), ArtistsAction::ShowInfo(Box::new(item)))
                                    }
                                    LibraryContextEntry::ShowInFolder => (
                                        Task::none(),
                                        ArtistsAction::ShowAlbumInFolder(album.id.clone()),
                                    ),
                                    LibraryContextEntry::Separator => {
                                        (Task::none(), ArtistsAction::None)
                                    }
                                    _ => (Task::none(), ArtistsAction::None),
                                },
                                Some(ThreeTierEntry::Grandchild(song, _)) => match entry {
                                    LibraryContextEntry::GetInfo => {
                                        use nokkvi_data::types::info_modal::InfoModalItem;
                                        let item = InfoModalItem::from_song_view_data(song);
                                        (Task::none(), ArtistsAction::ShowInfo(Box::new(item)))
                                    }
                                    LibraryContextEntry::ShowInFolder => (
                                        Task::none(),
                                        ArtistsAction::ShowSongInFolder(song.path.clone()),
                                    ),
                                    LibraryContextEntry::Separator => {
                                        (Task::none(), ArtistsAction::None)
                                    }
                                    _ => (Task::none(), ArtistsAction::None),
                                },
                                None => (Task::none(), ArtistsAction::None),
                            }
                        }
                    }
                }
                // Common arms already handled by macro above
                _ => (Task::none(), ArtistsAction::None),
            },
        }
    }

    // NOTE: build_flattened_list, collapse, clear are now on self.expansion (ExpansionState)

    /// Build the view
    pub fn view<'a>(&'a self, data: ArtistsViewData<'a>) -> Element<'a, ArtistsMessage> {
        use crate::widgets::view_header::SortMode;

        let header = widgets::view_header::view_header(
            self.common.current_sort_mode,
            SortMode::ARTIST_OPTIONS,
            self.common.sort_ascending,
            &self.common.search_query,
            data.artists.len(),
            data.total_artist_count,
            "artists",
            crate::views::ARTISTS_SEARCH_ID,
            ArtistsMessage::SortModeSelected,
            ArtistsMessage::ToggleSortOrder,
            None, // No shuffle button for artists
            Some(ArtistsMessage::RefreshViewData),
            Some(ArtistsMessage::CenterOnPlaying),
            ArtistsMessage::SearchQueryChanged,
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

        // If no artists match search, show message but keep the header
        if data.artists.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No artists match your search.",
                &layout_config,
            );
        }

        // Configure slot list with artists-specific chrome height (has view header)
        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_header, slot_list_view_with_scroll,
        };

        let config =
            SlotListConfig::with_dynamic_slots(data.window_height, chrome_height_with_header())
                .with_modifiers(data.modifiers);
        let artists = data.artists; // Borrow slice to extend lifetime
        let artist_art = data.artist_art;
        let current_sort_mode = self.common.current_sort_mode;

        // Build flattened list (artists + injected albums + injected tracks when expanded)
        let flattened = super::expansion::build_three_tier_list(
            artists,
            &self.expansion,
            &self.sub_expansion,
            |a| &a.id,
            |a| &a.id,
        );
        let center_index = self.common.get_center_item_index(flattened.len());

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            &flattened,
            &config,
            ArtistsMessage::SlotListNavigateUp,
            ArtistsMessage::SlotListNavigateDown,
            {
                let total = flattened.len();
                move |f| ArtistsMessage::SlotListScrollSeek((f * total as f32) as usize)
            },
            |entry, ctx| match entry {
                ThreeTierEntry::Parent(artist) => self.render_artist_row(
                    artist,
                    &ctx,
                    artist_art,
                    current_sort_mode,
                    data.stable_viewport,
                ),
                ThreeTierEntry::Child(album, _parent_artist_id) => {
                    self.render_album_child_row(album, &ctx, data.stable_viewport)
                }
                ThreeTierEntry::Grandchild(song, _album_id) => {
                    super::expansion::render_child_track_row(
                        song,
                        &ctx,
                        ArtistsMessage::SlotListActivateCenter,
                        if data.stable_viewport {
                            ArtistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
                        } else {
                            ArtistsMessage::SlotListClickPlay(ctx.item_index)
                        },
                        Some(ArtistsMessage::ClickToggleStar(ctx.item_index)),
                    )
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::{base_slot_list_layout, single_artwork_panel};

        // Build artwork column — show parent artist art even when on a child or grandchild
        let centered_artist = center_index.and_then(|idx| match flattened.get(idx) {
            Some(ThreeTierEntry::Parent(artist)) => {
                Some(artists.iter().find(|a| a.id == artist.id)?)
            }
            Some(ThreeTierEntry::Child(_, parent_id)) => {
                artists.iter().find(|a| &a.id == parent_id)
            }
            Some(ThreeTierEntry::Grandchild(_, _)) => {
                // grandchild: look up via sub_expansion parent (album) → outer expansion parent (artist)
                self.expansion
                    .expanded_id
                    .as_ref()
                    .and_then(|aid| artists.iter().find(|a| &a.id == aid))
            }
            None => None,
        });
        let artwork_handle = centered_artist.and_then(|artist| data.large_artwork.get(&artist.id));

        let artwork_content = Some(single_artwork_panel::<ArtistsMessage>(artwork_handle));

        base_slot_list_layout(&layout_config, header, slot_list_content, artwork_content)
    }

    /// Render an artist row in the slot list (standard layout)
    fn render_artist_row<'a>(
        &self,
        artist: &ArtistUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        artist_art: &'a HashMap<String, image::Handle>,
        current_sort_mode: widgets::view_header::SortMode,
        stable_viewport: bool,
    ) -> Element<'a, ArtistsMessage> {
        use crate::widgets::slot_list::{
            SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column, slot_list_text,
        };

        let artist_id = artist.id.clone();
        let artist_name = artist.name.clone();
        let album_count = artist.album_count;
        let song_count = artist.song_count;
        let is_starred = artist.is_starred;
        let rating = artist.rating.unwrap_or(0).min(5) as usize;

        // Check if this artist is the expanded one (gives it the group highlight)
        let is_expanded = self.expansion.is_expanded_parent(&artist.id);
        let style = SlotListSlotStyle::for_slot(
            ctx.is_center,
            is_expanded,
            ctx.is_selected,
            ctx.has_multi_selection,
            ctx.opacity,
        );

        let base_artwork_size = (ctx.row_height - 16.0).max(32.0);
        let artwork_size = base_artwork_size * ctx.scale_factor;
        let title_size =
            calculate_font_size(14.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
        let metadata_size =
            calculate_font_size(12.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;
        let star_size = (ctx.row_height * 0.3 * ctx.scale_factor).clamp(16.0, 24.0);
        let index_size =
            calculate_font_size(12.0, ctx.row_height, ctx.scale_factor) * ctx.scale_factor;

        // Layout: [Index] [Art] [Artist Name (50%)] [Album Count (22%)] [Song Count (21%)] [Star (5%)]
        let content = row![
            // 0. Index number (fixed width)
            slot_list_index_column(ctx.item_index, index_size, style, ctx.opacity),
            // 1. Artist Art (fixed width)
            {
                use crate::widgets::slot_list::slot_list_artwork_column;
                slot_list_artwork_column(
                    artist_art.get(&artist_id),
                    artwork_size,
                    ctx.is_center,
                    false,
                    ctx.opacity,
                )
            },
            // 2. Artist Name (50%) - with optional rating row
            {
                use iced::widget::column;
                let content: Element<'_, ArtistsMessage> =
                    if current_sort_mode == crate::widgets::view_header::SortMode::Rating {
                        use crate::widgets::slot_list::slot_list_star_rating;
                        let star_icon_size =
                            calculate_font_size(12.0, ctx.row_height, ctx.scale_factor)
                                * ctx.scale_factor;
                        let idx = ctx.item_index;

                        column![
                            slot_list_text(artist_name, title_size, style.text_color),
                            slot_list_star_rating(
                                rating,
                                star_icon_size,
                                ctx.is_center,
                                ctx.opacity,
                                None,
                                Some(move |star: usize| ArtistsMessage::ClickSetRating(idx, star)),
                            ),
                        ]
                        .spacing(2.0)
                        .into()
                    } else {
                        slot_list_text(artist_name, title_size, style.text_color).into()
                    };
                container(content)
                    .width(Length::FillPortion(50))
                    .height(Length::Fill)
                    .clip(true)
                    .align_y(Alignment::Center)
            },
            // 3. Album Count (22%)
            {
                use crate::widgets::slot_list::slot_list_metadata_column;
                let album_text = if album_count == 1 {
                    "1 album".to_string()
                } else {
                    format!("{album_count} albums")
                };
                slot_list_metadata_column(album_text, metadata_size, style, 22)
            },
            // 4. Song Count (21%)
            {
                use crate::widgets::slot_list::slot_list_metadata_column;
                slot_list_metadata_column(format!("{song_count} songs"), metadata_size, style, 21)
            },
            // 5. Star/Heart Icon (5%)
            container({
                use crate::widgets::slot_list::slot_list_favorite_icon;
                slot_list_favorite_icon(
                    is_starred,
                    ctx.is_center,
                    false,
                    ctx.opacity,
                    star_size,
                    "heart",
                    Some(ArtistsMessage::ClickToggleStar(ctx.item_index)),
                )
            })
            .width(Length::FillPortion(5))
            .padding(iced::Padding {
                left: 4.0,
                right: 4.0,
                ..Default::default()
            })
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
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

        // Wrap in clickable container
        let clickable = container(content)
            .style(move |_theme| style.to_container_style())
            .width(Length::Fill);

        let slot_button = button(clickable)
            .on_press(if ctx.modifiers.control() || ctx.modifiers.shift() {
                ArtistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else if ctx.is_center {
                ArtistsMessage::SlotListActivateCenter
            } else if stable_viewport {
                ArtistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                ArtistsMessage::SlotListClickPlay(ctx.item_index)
            })
            .style(|_theme, _status| button::Style {
                background: None,
                border: iced::Border::default(),
                ..Default::default()
            })
            .padding(0)
            .width(Length::Fill);

        use crate::widgets::context_menu::{
            context_menu, library_entries_with_folder, library_entry_view,
        };
        let item_idx = ctx.item_index;
        context_menu(
            slot_button,
            library_entries_with_folder(),
            move |entry, length| {
                library_entry_view(entry, length, |e| {
                    ArtistsMessage::ContextMenuAction(item_idx, e)
                })
            },
        )
        .into()
    }

    /// Render a child album row in the slot list (indented, simpler layout)
    fn render_album_child_row<'a>(
        &self,
        album: &AlbumUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        stable_viewport: bool,
    ) -> Element<'a, ArtistsMessage> {
        super::expansion::render_child_album_row(
            album,
            ctx,
            ArtistsMessage::SlotListActivateCenter,
            if stable_viewport {
                ArtistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                ArtistsMessage::SlotListClickPlay(ctx.item_index)
            },
            false, // artist is already the parent row
            Some(ArtistsMessage::ClickToggleStar(ctx.item_index)),
        )
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

impl super::ViewPage for ArtistsPage {
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
            // Inner collapse first
            Some(Message::Artists(ArtistsMessage::CollapseAlbumExpansion))
        } else {
            Some(Message::Artists(ArtistsMessage::CollapseExpansion))
        }
    }

    fn search_input_id(&self) -> &'static str {
        super::ARTISTS_SEARCH_ID
    }

    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        Some(SortMode::ARTIST_OPTIONS)
    }
    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Artists(ArtistsMessage::SortModeSelected(mode)))
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::Artists(ArtistsMessage::ToggleSortOrder)
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Artists(ArtistsMessage::AddCenterToQueue))
    }
    fn expand_center_message(&self) -> Option<Message> {
        if self.expansion.is_expanded() {
            // If albums are open, Shift+Enter now expands tracks on the centered album
            Some(Message::Artists(ArtistsMessage::ExpandAlbum))
        } else {
            Some(Message::Artists(ArtistsMessage::ExpandCenter))
        }
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadArtists)
    }
}
