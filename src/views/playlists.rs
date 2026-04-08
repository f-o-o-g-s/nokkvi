//! Playlists Page Component
//!
//! Self-contained playlists view with slot list navigation, search, and filtering.
//! Uses message bubbling pattern to communicate global actions to root.

use std::collections::HashMap;

use iced::{
    Alignment, Element, Length, Task,
    widget::{button, container, image},
};
use nokkvi_data::{
    backend::{playlists::PlaylistUIViewData, songs::SongUIViewData},
    utils::{formatters::format_date_concise, scale::calculate_font_size},
};

use super::expansion::{ExpansionState, SlotListEntry};
use crate::{
    app_message::Message,
    widgets::{self, SlotListPageState, view_header::SortMode},
};

/// Playlists page local state
#[derive(Debug)]
pub struct PlaylistsPage {
    pub common: SlotListPageState,
    pub expansion: ExpansionState<SongUIViewData>,
}

/// View data passed from root (read-only, borrows from app state to avoid allocations)
pub struct PlaylistsViewData<'a> {
    pub playlists: &'a [PlaylistUIViewData],
    pub playlist_artwork: &'a HashMap<String, image::Handle>,
    pub playlist_collage_artwork: &'a HashMap<String, Vec<image::Handle>>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    pub modifiers: iced::keyboard::Modifiers,
    pub total_playlist_count: usize,
    pub loading: bool,
    pub stable_viewport: bool,
}

/// Context menu entries for playlist parent items.
///
/// Extends the shared `LibraryContextEntry` with playlist-specific actions
/// (delete, rename). Uses a `Separator` variant for visual grouping.
#[derive(Debug, Clone, Copy)]
pub enum PlaylistContextEntry {
    /// Shared library entries (Play, AddToQueue, PlayNext)
    Library(crate::widgets::context_menu::LibraryContextEntry),
    /// Visual separator between shared and playlist-specific entries
    Separator,
    /// Delete this playlist
    Delete,
    /// Rename this playlist
    Rename,
    /// Edit playlist tracks in split-view
    EditPlaylist,
    /// Set this playlist as the default for quick-add
    SetAsDefault,
}

/// Build the full set of context menu entries for a playlist parent item.
fn playlist_context_entries() -> Vec<PlaylistContextEntry> {
    use crate::widgets::context_menu::LibraryContextEntry;
    vec![
        PlaylistContextEntry::Library(LibraryContextEntry::AddToQueue),
        PlaylistContextEntry::Separator,
        PlaylistContextEntry::EditPlaylist,
        PlaylistContextEntry::Rename,
        PlaylistContextEntry::Delete,
        PlaylistContextEntry::Separator,
        PlaylistContextEntry::SetAsDefault,
        PlaylistContextEntry::Library(LibraryContextEntry::GetInfo),
    ]
}

/// Render a playlist context menu entry.
fn playlist_entry_view<'a, Message: Clone + 'a>(
    entry: PlaylistContextEntry,
    length: Length,
    on_action: impl Fn(PlaylistContextEntry) -> Message,
) -> Element<'a, Message> {
    use crate::widgets::context_menu::{library_entry_view, menu_button, menu_separator};
    match entry {
        PlaylistContextEntry::Library(lib_entry) => library_entry_view(lib_entry, length, |e| {
            on_action(PlaylistContextEntry::Library(e))
        }),
        PlaylistContextEntry::Separator => menu_separator(),
        PlaylistContextEntry::Delete => menu_button(
            Some("assets/icons/trash-2.svg"),
            "Delete Playlist",
            on_action(PlaylistContextEntry::Delete),
        ),
        PlaylistContextEntry::Rename => menu_button(
            Some("assets/icons/pencil.svg"),
            "Rename Playlist",
            on_action(PlaylistContextEntry::Rename),
        ),
        PlaylistContextEntry::EditPlaylist => menu_button(
            Some("assets/icons/list.svg"),
            "Edit Playlist",
            on_action(PlaylistContextEntry::EditPlaylist),
        ),
        PlaylistContextEntry::SetAsDefault => menu_button(
            Some("assets/icons/star.svg"),
            "Set as Default Playlist",
            on_action(PlaylistContextEntry::SetAsDefault),
        ),
    }
}

/// Messages for local playlists page interactions
#[derive(Debug, Clone)]
pub enum PlaylistsMessage {
    // Slot list navigation
    SlotListNavigateUp,
    SlotListNavigateDown,
    SlotListSetOffset(usize, iced::keyboard::Modifiers),
    SlotListScrollSeek(usize),
    SlotListActivateCenter,
    SlotListClickPlay(usize), // Click non-center to play directly (skip focus)
    AddCenterToQueue,         // Add all songs from centered playlist to queue (Shift+A)

    // Mouse click on heart
    ClickToggleStar(usize), // item_index

    // Context menu (shared library entries for child tracks)
    ContextMenuAction(usize, crate::widgets::context_menu::LibraryContextEntry),
    /// Playlist-specific context menu action on a parent playlist
    PlaylistContextAction(usize, PlaylistContextEntry),

    // Expansion
    ExpandCenter,          // Toggle expand/collapse on centered playlist (Shift+Enter)
    FocusAndExpand(usize), // Clicked 'X songs' or playlist name — focus that row and expand it
    CollapseExpansion,     // Collapse current expansion (Escape when expanded)
    TracksLoaded(String, Vec<SongUIViewData>), // playlist_id, tracks

    // View header
    SortModeSelected(widgets::view_header::SortMode),
    ToggleSortOrder,
    SearchQueryChanged(String),
    SearchFocused(bool),
    RefreshViewData,

    // Data loading (moved from root Message enum)
    PlaylistsLoaded(Result<Vec<PlaylistUIViewData>, String>, usize), // result, total_count

    NavigateAndSearch(crate::View, String), // Navigate to target view and search
}

/// Actions that bubble up to root for global state mutation
#[derive(Debug, Clone)]
pub enum PlaylistsAction {
    PlayPlaylist(String), // playlist_id - clear queue and play all songs in playlist
    AddBatchToQueue(nokkvi_data::types::batch::BatchPayload),
    ExpandPlaylist(String), // playlist_id - load tracks for expansion
    PlayPlaylistFromTrack(String, usize), // playlist_id, track_index - play from clicked track
    LoadArtwork(String),    // playlist_id - load artwork for centered playlist on slot list scroll
    PreloadArtwork(usize),  // viewport_offset - preload artwork for visible + buffer
    SearchChanged(String),  // trigger reload
    SortModeChanged(widgets::view_header::SortMode), // trigger reload
    SortOrderChanged(bool), // trigger reload
    RefreshViewData,        // trigger reload
    ToggleStar(String, &'static str, bool), // (item_id, item_type, starred)
    PlayNextBatch(nokkvi_data::types::batch::BatchPayload),
    DeletePlaylist(String),               // playlist_id
    RenamePlaylist(String),               // playlist_id — triggers rename flow
    EditPlaylist(String, String, String), // (playlist_id, playlist_name, comment) — enter split-view edit mode
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>), // Open info modal
    SetAsDefaultPlaylist(String, String), // (playlist_id, playlist_name) — set as quick-add default
    NavigateAndSearch(crate::View, String), // Navigate to target view and search

    None,
}

impl super::HasCommonAction for PlaylistsAction {
    fn as_common(&self) -> super::CommonViewAction {
        match self {
            Self::SearchChanged(_) => super::CommonViewAction::SearchChanged,
            Self::SortModeChanged(m) => super::CommonViewAction::SortModeChanged(*m),
            Self::SortOrderChanged(a) => super::CommonViewAction::SortOrderChanged(*a),
            Self::RefreshViewData => super::CommonViewAction::RefreshViewData,
            Self::NavigateAndSearch(v, q) => {
                super::CommonViewAction::NavigateAndSearch(*v, q.clone())
            }

            Self::None => super::CommonViewAction::None,
            _ => super::CommonViewAction::ViewSpecific,
        }
    }
}

impl Default for PlaylistsPage {
    fn default() -> Self {
        Self {
            common: SlotListPageState::new(
                widgets::view_header::SortMode::Name,
                true, // sort_ascending
            ),
            expansion: ExpansionState::default(),
        }
    }
}

impl PlaylistsPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert sort mode to API string for server requests
    pub fn sort_mode_to_api_string(
        sort_mode: crate::widgets::view_header::SortMode,
    ) -> &'static str {
        match sort_mode {
            crate::widgets::view_header::SortMode::Name => "name",
            crate::widgets::view_header::SortMode::SongCount => "songCount",
            crate::widgets::view_header::SortMode::Duration => "duration",
            crate::widgets::view_header::SortMode::UpdatedAt => "updatedAt",
            crate::widgets::view_header::SortMode::Random => "random",
            _ => "name", // Default to name for unsupported types
        }
    }

    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: PlaylistsMessage,
        total_items: usize,
        playlists: &[PlaylistUIViewData],
    ) -> (Task<PlaylistsMessage>, PlaylistsAction) {
        match super::impl_expansion_update!(
            self, message, playlists, total_items,
            id_fn: |p| &p.id,
            expand_center: PlaylistsMessage::ExpandCenter => PlaylistsAction::ExpandPlaylist,
            collapse: PlaylistsMessage::CollapseExpansion,
            children_loaded: PlaylistsMessage::TracksLoaded,
            sort_selected: PlaylistsMessage::SortModeSelected => PlaylistsAction::SortModeChanged,
            toggle_sort: PlaylistsMessage::ToggleSortOrder => PlaylistsAction::SortOrderChanged,
            search_changed: PlaylistsMessage::SearchQueryChanged => PlaylistsAction::SearchChanged,
            search_focused: PlaylistsMessage::SearchFocused,
            action_none: PlaylistsAction::None,
        ) {
            Ok(result) => result,
            Err(msg) => match msg {
                PlaylistsMessage::SlotListNavigateUp => {
                    let center = self
                        .expansion
                        .handle_navigate_up(playlists, &mut self.common);
                    match center {
                        Some(idx) => (Task::none(), PlaylistsAction::LoadArtwork(idx.to_string())),
                        None => (Task::none(), PlaylistsAction::None),
                    }
                }
                PlaylistsMessage::SlotListNavigateDown => {
                    let center = self
                        .expansion
                        .handle_navigate_down(playlists, &mut self.common);
                    match center {
                        Some(idx) => (Task::none(), PlaylistsAction::LoadArtwork(idx.to_string())),
                        None => (Task::none(), PlaylistsAction::None),
                    }
                }
                PlaylistsMessage::SlotListSetOffset(offset, modifiers) => {
                    let center = self.expansion.handle_select_offset(
                        offset,
                        modifiers,
                        playlists,
                        &mut self.common,
                    );
                    match center {
                        Some(idx) => (Task::none(), PlaylistsAction::LoadArtwork(idx.to_string())),
                        None => (Task::none(), PlaylistsAction::None),
                    }
                }
                PlaylistsMessage::FocusAndExpand(idx) => {
                    self.common.slot_list.selected_indices.clear();
                    let (t1, _) = self.update(
                        PlaylistsMessage::SlotListSetOffset(
                            idx,
                            iced::keyboard::Modifiers::default(),
                        ),
                        total_items,
                        playlists,
                    );
                    let (t2, action) =
                        self.update(PlaylistsMessage::ExpandCenter, total_items, playlists);
                    (Task::batch(vec![t1, t2]), action)
                }
                PlaylistsMessage::SlotListScrollSeek(offset) => {
                    self.expansion
                        .handle_set_offset(offset, playlists, &mut self.common);
                    (Task::none(), PlaylistsAction::None)
                }
                PlaylistsMessage::SlotListClickPlay(offset) => {
                    self.expansion
                        .handle_set_offset(offset, playlists, &mut self.common);
                    self.update(
                        PlaylistsMessage::SlotListActivateCenter,
                        total_items,
                        playlists,
                    )
                }
                PlaylistsMessage::SlotListActivateCenter => {
                    let total = self.expansion.flattened_len(playlists);
                    if let Some(center_idx) = self.common.get_center_item_index(total) {
                        self.common.slot_list.flash_center();
                        match self
                            .expansion
                            .get_entry_at(center_idx, playlists, |p| &p.id)
                        {
                            Some(SlotListEntry::Child(_song, parent_playlist_id)) => {
                                // Play playlist starting from this track
                                let track_idx = self.expansion.count_children_before(
                                    center_idx,
                                    playlists,
                                    |p| &p.id,
                                );
                                (
                                    Task::none(),
                                    PlaylistsAction::PlayPlaylistFromTrack(
                                        parent_playlist_id,
                                        track_idx,
                                    ),
                                )
                            }
                            Some(SlotListEntry::Parent(playlist)) => (
                                Task::none(),
                                PlaylistsAction::PlayPlaylist(playlist.id.clone()),
                            ),
                            None => (Task::none(), PlaylistsAction::None),
                        }
                    } else {
                        (Task::none(), PlaylistsAction::None)
                    }
                }
                PlaylistsMessage::AddCenterToQueue => {
                    use nokkvi_data::types::batch::BatchItem;
                    let total = self.expansion.flattened_len(playlists);

                    let target_indices = self.common.get_queue_target_indices(total);

                    if target_indices.is_empty() {
                        return (Task::none(), PlaylistsAction::None);
                    }

                    let payload = super::expansion::build_batch_payload(target_indices, |i| {
                        match self.expansion.get_entry_at(i, playlists, |p| &p.id) {
                            Some(SlotListEntry::Parent(playlist)) => {
                                Some(BatchItem::Playlist(playlist.id.clone()))
                            }
                            Some(SlotListEntry::Child(song, _)) => {
                                let item: nokkvi_data::types::song::Song = song.clone().into();
                                Some(BatchItem::Song(Box::new(item)))
                            }
                            None => None,
                        }
                    });

                    (Task::none(), PlaylistsAction::AddBatchToQueue(payload))
                }
                PlaylistsMessage::ClickToggleStar(item_index) => {
                    if let Some(entry) = self
                        .expansion
                        .get_entry_at(item_index, playlists, |p| &p.id)
                    {
                        match entry {
                            SlotListEntry::Child(song, _) => (
                                Task::none(),
                                PlaylistsAction::ToggleStar(
                                    song.id.clone(),
                                    "song",
                                    !song.is_starred,
                                ),
                            ),
                            SlotListEntry::Parent(_playlist) => {
                                // Playlists don't have starred state
                                (Task::none(), PlaylistsAction::None)
                            }
                        }
                    } else {
                        (Task::none(), PlaylistsAction::None)
                    }
                }
                // Data loading messages (handled at root level, no action needed here)
                PlaylistsMessage::PlaylistsLoaded(_, _) => (Task::none(), PlaylistsAction::None),
                PlaylistsMessage::RefreshViewData => {
                    (Task::none(), PlaylistsAction::RefreshViewData)
                }
                PlaylistsMessage::NavigateAndSearch(view, query) => (
                    Task::none(),
                    PlaylistsAction::NavigateAndSearch(view, query),
                ),

                PlaylistsMessage::ContextMenuAction(clicked_idx, entry) => {
                    // Context menu for child tracks (uses shared LibraryContextEntry)
                    use nokkvi_data::types::batch::BatchItem;

                    use crate::widgets::context_menu::LibraryContextEntry;

                    if matches!(
                        entry,
                        LibraryContextEntry::AddToQueue | LibraryContextEntry::AddToPlaylist
                    ) {
                        let target_indices = self.common.get_batch_target_indices(clicked_idx);
                        let payload = super::expansion::build_batch_payload(target_indices, |i| {
                            match self.expansion.get_entry_at(i, playlists, |p| &p.id) {
                                Some(SlotListEntry::Parent(playlist)) => {
                                    Some(BatchItem::Playlist(playlist.id.clone()))
                                }
                                Some(SlotListEntry::Child(song, _)) => {
                                    let item: nokkvi_data::types::song::Song = song.clone().into();
                                    Some(BatchItem::Song(Box::new(item)))
                                }
                                None => None,
                            }
                        });

                        match entry {
                            LibraryContextEntry::AddToQueue => {
                                return (Task::none(), PlaylistsAction::AddBatchToQueue(payload));
                            }
                            LibraryContextEntry::AddToPlaylist => {
                                return (Task::none(), PlaylistsAction::None); // Handle AddToPlaylist if needed later, right now playlists might not be addable into playlists?
                            }
                            _ => unreachable!(),
                        }
                    }

                    match self
                        .expansion
                        .get_entry_at(clicked_idx, playlists, |p| &p.id)
                    {
                        Some(SlotListEntry::Child(song, _)) => match entry {
                            LibraryContextEntry::GetInfo => {
                                use nokkvi_data::types::info_modal::InfoModalItem;
                                let item = InfoModalItem::from_song_view_data(song);
                                (Task::none(), PlaylistsAction::ShowInfo(Box::new(item)))
                            }
                            _ => (Task::none(), PlaylistsAction::None),
                        },
                        _ => (Task::none(), PlaylistsAction::None),
                    }
                }
                PlaylistsMessage::PlaylistContextAction(clicked_idx, entry) => {
                    // Context menu for parent playlists (extended entries)
                    use nokkvi_data::types::batch::BatchItem;

                    use crate::widgets::context_menu::LibraryContextEntry;

                    if matches!(
                        entry,
                        PlaylistContextEntry::Library(
                            LibraryContextEntry::AddToQueue | LibraryContextEntry::AddToPlaylist
                        )
                    ) {
                        let target_indices = self.common.get_batch_target_indices(clicked_idx);
                        let payload = super::expansion::build_batch_payload(target_indices, |i| {
                            match self.expansion.get_entry_at(i, playlists, |p| &p.id) {
                                Some(SlotListEntry::Parent(playlist)) => {
                                    Some(BatchItem::Playlist(playlist.id.clone()))
                                }
                                Some(SlotListEntry::Child(song, _)) => {
                                    let item: nokkvi_data::types::song::Song = song.clone().into();
                                    Some(BatchItem::Song(Box::new(item)))
                                }
                                None => None,
                            }
                        });

                        match entry {
                            PlaylistContextEntry::Library(LibraryContextEntry::AddToQueue) => {
                                return (Task::none(), PlaylistsAction::AddBatchToQueue(payload));
                            }
                            PlaylistContextEntry::Library(LibraryContextEntry::AddToPlaylist) => {
                                return (Task::none(), PlaylistsAction::None);
                            }
                            _ => unreachable!(),
                        }
                    }

                    match self
                        .expansion
                        .get_entry_at(clicked_idx, playlists, |p| &p.id)
                    {
                        Some(SlotListEntry::Parent(playlist)) => match entry {
                            PlaylistContextEntry::Delete => (
                                Task::none(),
                                PlaylistsAction::DeletePlaylist(playlist.id.clone()),
                            ),
                            PlaylistContextEntry::Rename => (
                                Task::none(),
                                PlaylistsAction::RenamePlaylist(playlist.id.clone()),
                            ),
                            PlaylistContextEntry::EditPlaylist => (
                                Task::none(),
                                PlaylistsAction::EditPlaylist(
                                    playlist.id.clone(),
                                    playlist.name.clone(),
                                    playlist.comment.clone(),
                                ),
                            ),
                            PlaylistContextEntry::SetAsDefault => (
                                Task::none(),
                                PlaylistsAction::SetAsDefaultPlaylist(
                                    playlist.id.clone(),
                                    playlist.name.clone(),
                                ),
                            ),
                            PlaylistContextEntry::Library(LibraryContextEntry::GetInfo) => {
                                use nokkvi_data::types::info_modal::InfoModalItem;
                                let item = InfoModalItem::Playlist {
                                    name: playlist.name.clone(),
                                    comment: playlist.comment.clone(),
                                    duration: playlist.duration,
                                    song_count: playlist.song_count,
                                    size: 0, // Not available on PlaylistUIViewData
                                    owner_name: playlist.owner_name.clone(),
                                    public: playlist.public,
                                    created_at: String::new(), // Not available on PlaylistUIViewData
                                    updated_at: playlist.updated_at.clone(),
                                    id: playlist.id.clone(),
                                };
                                (Task::none(), PlaylistsAction::ShowInfo(Box::new(item)))
                            }
                            _ => (Task::none(), PlaylistsAction::None),
                        },
                        _ => (Task::none(), PlaylistsAction::None),
                    }
                }
                // Common arms already handled by macro above
                _ => (Task::none(), PlaylistsAction::None),
            },
        }
    }

    /// Build the view
    pub fn view<'a>(&'a self, data: PlaylistsViewData<'a>) -> Element<'a, PlaylistsMessage> {
        use crate::widgets::view_header::SortMode;

        let header = widgets::view_header::view_header(
            self.common.current_sort_mode,
            SortMode::PLAYLIST_OPTIONS,
            self.common.sort_ascending,
            &self.common.search_query,
            data.playlists.len(),
            data.total_playlist_count,
            "playlists",
            crate::views::PLAYLISTS_SEARCH_ID,
            PlaylistsMessage::SortModeSelected,
            Some(PlaylistsMessage::ToggleSortOrder),
            None, // No shuffle button for playlists
            Some(PlaylistsMessage::RefreshViewData),
            None, // Playlists view doesn't need center on playing button
            true, // show_search
            PlaylistsMessage::SearchQueryChanged,
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

        // If no playlists match search, show message but keep the header
        if data.playlists.is_empty() {
            return widgets::base_slot_list_empty_state(
                header,
                "No playlists match your search.",
                &layout_config,
            );
        }

        // Configure slot list with playlists-specific chrome height (has view header)
        use crate::widgets::slot_list::{
            SlotListConfig, chrome_height_with_header, slot_list_view_with_scroll,
        };

        let config =
            SlotListConfig::with_dynamic_slots(data.window_height, chrome_height_with_header())
                .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let playlists = data.playlists; // Borrow slice to extend lifetime
        let playlist_artwork = data.playlist_artwork;
        let playlist_collage_artwork = data.playlist_collage_artwork;

        // Build flattened list (playlists + injected tracks when expanded)
        let flattened = self.expansion.build_flattened_list(playlists, |p| &p.id);
        let center_index = self.common.get_center_item_index(flattened.len());

        // Render slot list using generic component with item renderer closure
        let slot_list_content = slot_list_view_with_scroll(
            &self.common.slot_list,
            &flattened,
            &config,
            PlaylistsMessage::SlotListNavigateUp,
            PlaylistsMessage::SlotListNavigateDown,
            {
                let total = flattened.len();
                move |f| PlaylistsMessage::SlotListScrollSeek((f * total as f32) as usize)
            },
            |entry, ctx| match entry {
                SlotListEntry::Parent(playlist) => {
                    self.render_playlist_row(playlist, &ctx, playlist_artwork, data.stable_viewport)
                }
                SlotListEntry::Child(song, _parent_playlist_id) => {
                    self.render_track_row(song, &ctx, data.stable_viewport)
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

        use crate::widgets::base_slot_list_layout::{
            base_slot_list_layout, collage_artwork_panel, single_artwork_panel,
        };

        // Build artwork column — show parent playlist art even when on a child track
        let centered_playlist = center_index.and_then(|idx| match flattened.get(idx) {
            Some(SlotListEntry::Parent(playlist)) => Some(playlist),
            Some(SlotListEntry::Child(_, parent_id)) => {
                playlists.iter().find(|p| &p.id == parent_id)
            }
            None => None,
        });
        let playlist_id = centered_playlist.map(|p| p.id.clone()).unwrap_or_default();

        // Get collage handles for centered playlist (borrow, don't clone)
        let collage_handles = playlist_collage_artwork.get(&playlist_id);

        // Show single full-res when 0-1 albums, collage when 2+ albums
        let album_count = centered_playlist.map_or(0, |p| p.artwork_album_ids.len());

        let artwork_content = if album_count <= 1 {
            // Show single artwork full-size (use collage[0] if available, else mini)
            let handle = collage_handles
                .and_then(|v| v.first())
                .or_else(|| playlist_artwork.get(&playlist_id));
            Some(single_artwork_panel::<PlaylistsMessage>(handle))
        } else if let Some(handles) = collage_handles.filter(|v| !v.is_empty()) {
            // Render 3x3 collage grid (2+ albums)
            Some(collage_artwork_panel::<PlaylistsMessage>(handles))
        } else {
            // album_count > 1 but collage NOT loaded yet - show placeholder
            Some(single_artwork_panel::<PlaylistsMessage>(None))
        };

        base_slot_list_layout(&layout_config, header, slot_list_content, artwork_content)
    }

    /// Render a parent playlist row in the slot list
    fn render_playlist_row<'a>(
        &self,
        playlist: &PlaylistUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        playlist_artwork: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
    ) -> Element<'a, PlaylistsMessage> {
        use crate::widgets::slot_list::{
            SLOT_LIST_SLOT_PADDING, SlotListSlotStyle, slot_list_index_column,
        };

        let is_expanded = self.expansion.is_expanded_parent(&playlist.id);
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

        // Format duration
        let duration_mins = (playlist.duration / 60.0) as u32;
        let duration_str = if duration_mins < 60 {
            format!("{duration_mins} min")
        } else {
            let hours = duration_mins / 60;
            let mins = duration_mins % 60;
            format!("{hours}h {mins}m")
        };

        let sort_mode = self.common.current_sort_mode;
        let show_song_count_col = sort_mode == SortMode::SongCount;
        let show_duration_col = sort_mode == SortMode::Duration;
        let show_updated_at = sort_mode == SortMode::UpdatedAt;

        // Build subtitle: owner + song count/duration (unless they have their own column)
        let count_text = if playlist.song_count == 1 {
            "1 song".to_string()
        } else {
            format!("{} songs", playlist.song_count)
        };

        let mut subtitle_parts: Vec<&str> = Vec::new();
        if !show_song_count_col {
            subtitle_parts.push(&count_text);
        }
        if !show_duration_col {
            subtitle_parts.push(&duration_str);
        }
        let subtitle = subtitle_parts.join(" · ");

        // Extra columns reduce the name portion to make room
        let extra_cols =
            show_song_count_col as u16 + show_duration_col as u16 + show_updated_at as u16;
        let name_portion = 55 - extra_cols * 10;

        // Layout: [Index] [Artwork] [Name+subtitle] [SongCount?] [Duration?] [UpdatedAt?]
        use crate::widgets::slot_list::{slot_list_artwork_column, slot_list_metadata_column};

        let mut columns: Vec<Element<'a, PlaylistsMessage>> = vec![
            slot_list_index_column(ctx.item_index, index_size, style, ctx.opacity),
            slot_list_artwork_column(
                playlist_artwork.get(&playlist.id),
                artwork_size,
                ctx.is_center,
                false,
                ctx.opacity,
            ),
            {
                let click_title = Some(PlaylistsMessage::PlaylistContextAction(
                    ctx.item_index,
                    PlaylistContextEntry::Library(
                        crate::widgets::context_menu::LibraryContextEntry::GetInfo,
                    ),
                ));
                use crate::widgets::slot_list::slot_list_text_column;
                slot_list_text_column(
                    playlist.name.clone(),
                    click_title,
                    subtitle,
                    Some(PlaylistsMessage::FocusAndExpand(ctx.item_index)),
                    title_size,
                    metadata_size,
                    style,
                    ctx.is_center,
                    name_portion,
                )
            },
        ];

        if show_song_count_col {
            columns.push(slot_list_metadata_column(
                count_text.clone(),
                Some(PlaylistsMessage::FocusAndExpand(ctx.item_index)),
                metadata_size,
                style,
                20,
            ));
        }
        if show_duration_col {
            columns.push(slot_list_metadata_column(
                duration_str.clone(),
                None,
                metadata_size,
                style,
                20,
            ));
        }
        if show_updated_at {
            let date_str = format_date_concise(&playlist.updated_at);
            columns.push(slot_list_metadata_column(
                date_str,
                None,
                metadata_size,
                style,
                20,
            ));
        }

        let content = iced::widget::Row::with_children(columns)
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
                PlaylistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else if ctx.is_center {
                PlaylistsMessage::SlotListActivateCenter
            } else if stable_viewport {
                PlaylistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                PlaylistsMessage::SlotListClickPlay(ctx.item_index)
            })
            .style(|_theme, _status| button::Style {
                background: None,
                border: iced::Border::default(),
                ..Default::default()
            })
            .padding(0)
            .width(Length::Fill);

        use crate::widgets::context_menu::context_menu;
        let item_idx = ctx.item_index;
        context_menu(
            slot_button,
            playlist_context_entries(),
            move |entry, length| {
                playlist_entry_view(entry, length, |e| {
                    PlaylistsMessage::PlaylistContextAction(item_idx, e)
                })
            },
        )
        .into()
    }

    /// Render a child track row in the slot list (indented, simpler layout)
    fn render_track_row<'a>(
        &self,
        song: &SongUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        stable_viewport: bool,
    ) -> Element<'a, PlaylistsMessage> {
        super::expansion::render_child_track_row(
            song,
            ctx,
            PlaylistsMessage::SlotListActivateCenter,
            if stable_viewport {
                PlaylistsMessage::SlotListSetOffset(ctx.item_index, ctx.modifiers)
            } else {
                PlaylistsMessage::SlotListClickPlay(ctx.item_index)
            },
            Some(PlaylistsMessage::ClickToggleStar(ctx.item_index)),
            Some(PlaylistsMessage::NavigateAndSearch(
                crate::View::Artists,
                song.artist.clone(),
            )),
            1, // depth 1: child tracks under playlist
        )
    }
}

// ============================================================================
// ViewPage trait implementation
// ============================================================================

impl super::ViewPage for PlaylistsPage {
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
        Some(Message::Playlists(PlaylistsMessage::CollapseExpansion))
    }

    fn search_input_id(&self) -> &'static str {
        super::PLAYLISTS_SEARCH_ID
    }

    fn sort_mode_options(&self) -> Option<&'static [SortMode]> {
        Some(SortMode::PLAYLIST_OPTIONS)
    }
    fn sort_mode_selected_message(&self, mode: SortMode) -> Option<Message> {
        Some(Message::Playlists(PlaylistsMessage::SortModeSelected(mode)))
    }
    fn toggle_sort_order_message(&self) -> Message {
        Message::Playlists(PlaylistsMessage::ToggleSortOrder)
    }

    fn add_to_queue_message(&self) -> Option<Message> {
        Some(Message::Playlists(PlaylistsMessage::AddCenterToQueue))
    }
    fn expand_center_message(&self) -> Option<Message> {
        Some(Message::Playlists(PlaylistsMessage::ExpandCenter))
    }
    fn reload_message(&self) -> Option<Message> {
        Some(Message::LoadPlaylists)
    }
}
