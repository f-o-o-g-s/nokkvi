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
    utils::formatters::format_date_concise,
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
    /// Per-column visibility toggles surfaced via the columns-cog dropdown.
    pub column_visibility: PlaylistsColumnVisibility,
}

/// Toggleable playlists columns. The playlist name (title) is always shown;
/// SongCount/Duration/UpdatedAt also auto-show when their matching sort
/// mode is active regardless of the user toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistsColumn {
    Select,
    Index,
    Thumbnail,
    SongCount,
    Duration,
    UpdatedAt,
}

/// User-toggle state for each toggleable playlists column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaylistsColumnVisibility {
    pub select: bool,
    pub index: bool,
    pub thumbnail: bool,
    pub songcount: bool,
    pub duration: bool,
    pub updatedat: bool,
}

impl Default for PlaylistsColumnVisibility {
    fn default() -> Self {
        // Index/Thumbnail historically always render. SongCount/Duration/
        // UpdatedAt historically only render when sorting by the matching
        // mode — keep that as the default to avoid surprising layout
        // changes on first launch. Select defaults off — opt-in discovery
        // affordance for multi-selection.
        Self {
            select: false,
            index: true,
            thumbnail: true,
            songcount: false,
            duration: false,
            updatedat: false,
        }
    }
}

impl PlaylistsColumnVisibility {
    pub fn get(&self, col: PlaylistsColumn) -> bool {
        match col {
            PlaylistsColumn::Select => self.select,
            PlaylistsColumn::Index => self.index,
            PlaylistsColumn::Thumbnail => self.thumbnail,
            PlaylistsColumn::SongCount => self.songcount,
            PlaylistsColumn::Duration => self.duration,
            PlaylistsColumn::UpdatedAt => self.updatedat,
        }
    }

    pub fn set(&mut self, col: PlaylistsColumn, value: bool) {
        match col {
            PlaylistsColumn::Select => self.select = value,
            PlaylistsColumn::Index => self.index = value,
            PlaylistsColumn::Thumbnail => self.thumbnail = value,
            PlaylistsColumn::SongCount => self.songcount = value,
            PlaylistsColumn::Duration => self.duration = value,
            PlaylistsColumn::UpdatedAt => self.updatedat = value,
        }
    }
}

/// SongCount column auto-shows when sort = SongCount regardless of toggle.
pub(crate) fn playlists_song_count_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::SongCount)
}

/// Duration column auto-shows when sort = Duration regardless of toggle.
pub(crate) fn playlists_duration_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::Duration)
}

/// UpdatedAt column auto-shows when sort = UpdatedAt regardless of toggle.
pub(crate) fn playlists_updated_at_visible(sort: SortMode, user_visible: bool) -> bool {
    user_visible || matches!(sort, SortMode::UpdatedAt)
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
    /// Current default playlist's display name (empty when no default set).
    /// Surfaced in the view-header chip.
    pub default_playlist_name: &'a str,
    /// Whether the column-visibility checkbox dropdown is open (controlled
    /// by `Nokkvi.open_menu`).
    pub column_dropdown_open: bool,
    /// Trigger bounds captured when the dropdown was opened.
    pub column_dropdown_trigger_bounds: Option<iced::Rectangle>,
    /// Borrowed reference to the root open-menu state, so per-row context
    /// menus and the artwork-panel context menu can resolve their own
    /// open/closed status.
    pub open_menu: Option<&'a crate::app_message::OpenMenu>,
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
    /// Click on a row's leading select checkbox — toggles `item_index` in
    /// `selected_indices`. No play/highlight side effects.
    SlotListSelectionToggle(usize),
    /// Click on the tri-state "select all" header — fills selection with
    /// every visible row, or clears if every visible row is already selected.
    SlotListSelectAllToggle,
    AddCenterToQueue, // Add all songs from centered playlist to queue (Shift+A)

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

    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter), // Navigate to target view and filter
    /// Navigate to Artists and auto-expand the artist with this id (no filter set).
    NavigateAndExpandArtist(String),

    /// Context-menu open/close request — bubbled to root
    /// `Message::SetOpenMenu`. Intercepted in `handle_playlists` before the
    /// page's `update` runs.
    SetOpenMenu(Option<crate::app_message::OpenMenu>),
    /// Artwork column drag handle event — intercepted at root, page never sees it.
    ArtworkColumnDrag(crate::widgets::artwork_split_handle::DragEvent),
    /// Header chip clicked — bubble to root, opens the default-playlist picker.
    OpenDefaultPlaylistPicker,
    /// View-header `+` button clicked — bubble to root to open the
    /// Create-New-Playlist dialog.
    OpenCreatePlaylistDialog,
    /// Toggle a playlists column's visibility.
    ToggleColumnVisible(PlaylistsColumn),
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
    DeletePlaylist(String),                     // playlist_id
    RenamePlaylist(String),                     // playlist_id — triggers rename flow
    EditPlaylist(String, String, String, bool), // (playlist_id, playlist_name, comment, public) — enter split-view edit mode
    ShowInfo(Box<nokkvi_data::types::info_modal::InfoModalItem>), // Open info modal
    SetAsDefaultPlaylist(String, String), // (playlist_id, playlist_name) — set as quick-add default
    NavigateAndFilter(crate::View, nokkvi_data::types::filter::LibraryFilter), // Navigate to target view and filter
    NavigateAndExpandArtist(String), // artist_id - navigate to Artists and auto-expand
    /// Bubble to root: open the default-playlist picker overlay.
    OpenDefaultPlaylistPicker,
    /// Bubble to root: open the Create-New-Playlist dialog.
    OpenCreatePlaylistDialog,
    /// Persist a column-visibility toggle change (col, new_value).
    ColumnVisibilityChanged(PlaylistsColumn, bool),

    None,
}

impl super::HasCommonAction for PlaylistsAction {
    fn as_common(&self) -> super::CommonViewAction {
        match self {
            Self::SearchChanged(_) => super::CommonViewAction::SearchChanged,
            Self::SortModeChanged(m) => super::CommonViewAction::SortModeChanged(*m),
            Self::SortOrderChanged(a) => super::CommonViewAction::SortOrderChanged(*a),
            Self::RefreshViewData => super::CommonViewAction::RefreshViewData,
            Self::NavigateAndFilter(v, f) => {
                super::CommonViewAction::NavigateAndFilter(*v, f.clone())
            }
            Self::NavigateAndExpandArtist(id) => {
                super::CommonViewAction::NavigateAndExpandArtist(id.clone())
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
            column_visibility: PlaylistsColumnVisibility::default(),
        }
    }
}

impl PlaylistsPage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert sort mode to API string for server requests.
    /// Thin shim — the unified mapping lives in `views/sort_api.rs`.
    pub fn sort_mode_to_api_string(
        sort_mode: crate::widgets::view_header::SortMode,
    ) -> &'static str {
        super::sort_api::sort_mode_to_api_string(crate::View::Playlists, sort_mode)
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
                PlaylistsMessage::SlotListSelectionToggle(offset) => {
                    self.common.handle_selection_toggle(offset, total_items);
                    (Task::none(), PlaylistsAction::None)
                }
                PlaylistsMessage::SlotListSelectAllToggle => {
                    self.common.handle_select_all_toggle(total_items);
                    (Task::none(), PlaylistsAction::None)
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
                // Routed up to root in `handle_playlists` before this match
                // runs; arm exists only for exhaustiveness.
                PlaylistsMessage::SetOpenMenu(_) => (Task::none(), PlaylistsAction::None),
                PlaylistsMessage::RefreshViewData => {
                    (Task::none(), PlaylistsAction::RefreshViewData)
                }
                PlaylistsMessage::NavigateAndFilter(view, filter) => (
                    Task::none(),
                    PlaylistsAction::NavigateAndFilter(view, filter),
                ),
                PlaylistsMessage::NavigateAndExpandArtist(artist_id) => (
                    Task::none(),
                    PlaylistsAction::NavigateAndExpandArtist(artist_id),
                ),

                PlaylistsMessage::OpenDefaultPlaylistPicker => {
                    (Task::none(), PlaylistsAction::OpenDefaultPlaylistPicker)
                }
                PlaylistsMessage::OpenCreatePlaylistDialog => {
                    (Task::none(), PlaylistsAction::OpenCreatePlaylistDialog)
                }
                PlaylistsMessage::ToggleColumnVisible(col) => {
                    let new_value = !self.column_visibility.get(col);
                    self.column_visibility.set(col, new_value);
                    (
                        Task::none(),
                        PlaylistsAction::ColumnVisibilityChanged(col, new_value),
                    )
                }

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
                                    playlist.public,
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

        let chip: Element<'a, PlaylistsMessage> =
            crate::widgets::default_playlist_chip::default_playlist_chip(
                data.default_playlist_name,
                PlaylistsMessage::OpenDefaultPlaylistPicker,
            );

        let column_dropdown: Element<'a, PlaylistsMessage> = {
            use crate::widgets::checkbox_dropdown::checkbox_dropdown;
            let items: Vec<(PlaylistsColumn, &'static str, bool)> = vec![
                (
                    PlaylistsColumn::Select,
                    "Select",
                    self.column_visibility.select,
                ),
                (
                    PlaylistsColumn::Index,
                    "Index",
                    self.column_visibility.index,
                ),
                (
                    PlaylistsColumn::Thumbnail,
                    "Thumbnail",
                    self.column_visibility.thumbnail,
                ),
                (
                    PlaylistsColumn::SongCount,
                    "Song count",
                    self.column_visibility.songcount,
                ),
                (
                    PlaylistsColumn::Duration,
                    "Duration",
                    self.column_visibility.duration,
                ),
                (
                    PlaylistsColumn::UpdatedAt,
                    "Updated at",
                    self.column_visibility.updatedat,
                ),
            ];
            checkbox_dropdown(
                "assets/icons/columns-3-cog.svg",
                "Show/hide columns",
                items,
                PlaylistsMessage::ToggleColumnVisible,
                |trigger_bounds| match trigger_bounds {
                    Some(b) => PlaylistsMessage::SetOpenMenu(Some(
                        crate::app_message::OpenMenu::CheckboxDropdown {
                            view: crate::View::Playlists,
                            trigger_bounds: b,
                        },
                    )),
                    None => PlaylistsMessage::SetOpenMenu(None),
                },
                data.column_dropdown_open,
                data.column_dropdown_trigger_bounds,
            )
            .into()
        };

        // Header's trailing slot only takes one element — bundle the
        // existing default-playlist chip with the new columns-cog into a
        // small Row so both render side-by-side.
        let trailing: Element<'a, PlaylistsMessage> = iced::widget::row![chip, column_dropdown]
            .spacing(6)
            .align_y(Alignment::Center)
            .into();

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
            Some(("New Playlist", PlaylistsMessage::OpenCreatePlaylistDialog)),
            Some(trailing), // chip + columns-cog dropdown
            true,           // show_search
            PlaylistsMessage::SearchQueryChanged,
        );

        // Compose with the tri-state "select all" header bar when the
        // multi-select column is on. Tri-state derives from the current
        // selection set against the *flattened* (visible) row count.
        let header = {
            let flattened_len = self
                .expansion
                .build_flattened_list(data.playlists, |p| &p.id)
                .len();
            crate::widgets::slot_list::compose_header_with_select(
                self.column_visibility.select,
                self.common.select_all_state(flattened_len),
                PlaylistsMessage::SlotListSelectAllToggle,
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
            SlotListConfig, chrome_height_with_select_header, slot_list_view_with_scroll,
        };

        let select_header_visible = self.column_visibility.select;
        let config = SlotListConfig::with_dynamic_slots(
            data.window_height,
            chrome_height_with_select_header(select_header_visible),
        )
        .with_modifiers(data.modifiers);

        // Capture values needed in closure
        let playlists = data.playlists; // Borrow slice to extend lifetime
        let playlist_artwork = data.playlist_artwork;
        let playlist_collage_artwork = data.playlist_collage_artwork;
        let open_menu_for_rows = data.open_menu;

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
                    let row = self.render_playlist_row(
                        playlist,
                        &ctx,
                        playlist_artwork,
                        data.stable_viewport,
                        open_menu_for_rows,
                    );
                    crate::widgets::slot_list::wrap_with_select_column(
                        select_header_visible,
                        ctx.is_selected,
                        ctx.item_index,
                        PlaylistsMessage::SlotListSelectionToggle,
                        row,
                    )
                }
                SlotListEntry::Child(song, _parent_playlist_id) => {
                    self.render_track_row(song, &ctx, data.stable_viewport)
                }
            },
        );

        // Wrap slot list content with standard background (prevents color bleed-through)
        use crate::widgets::slot_list::slot_list_background_container;
        let slot_list_content = slot_list_background_container(slot_list_content);

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

        let pill_content = centered_playlist
            .filter(|_| crate::theme::playlists_artwork_overlay())
            .map(|playlist| {
                use iced::widget::{column, text};

                use crate::theme;

                let mut col = column![
                    text(playlist.name.clone())
                        .size(24)
                        .font(iced::Font {
                            weight: iced::font::Weight::Bold,
                            ..theme::ui_font()
                        })
                        .color(theme::fg0()),
                ]
                .spacing(4)
                .align_x(iced::Alignment::Center);

                if !playlist.comment.is_empty() {
                    let comment = &playlist.comment;
                    let preview: String = comment.chars().take(100).collect();
                    let preview = if comment.chars().count() > 100 {
                        format!("{}...", preview.trim_end())
                    } else {
                        preview
                    };
                    col = col.push(
                        text(preview)
                            .size(14)
                            .color(theme::fg2())
                            .font(theme::ui_font())
                            .center(),
                    );
                }

                let duration_min = playlist.duration / 60.0;
                let mut stats = vec![
                    format!("{} songs", playlist.song_count),
                    format!("{} mins", duration_min.round()),
                ];
                let ymd = playlist
                    .updated_at
                    .split('T')
                    .next()
                    .unwrap_or(&playlist.updated_at);
                stats.push(format!("Updated: {ymd}"));

                use crate::widgets::metadata_pill::dot_row;
                if let Some(row) = dot_row::<PlaylistsMessage>(stats, 13.0, theme::fg3()) {
                    col = col.push(row);
                }

                col.into()
            });

        use crate::widgets::base_slot_list_layout::{
            collage_artwork_panel_with_pill, single_artwork_panel_with_pill,
        };

        // Playlist artwork panels currently have no refresh action wired up,
        // but the helper still requires the controlled-component plumbing.
        // Pass inert defaults — no menu opens because `on_refresh` is None.
        let artwork_content = if album_count <= 1 {
            // Show single artwork full-size (use collage[0] if available, else mini)
            let handle = collage_handles
                .and_then(|v| v.first())
                .or_else(|| playlist_artwork.get(&playlist_id));
            Some(single_artwork_panel_with_pill::<PlaylistsMessage>(
                handle,
                pill_content,
                None, // Use standard dark backdrop
                None,
                false,
                None,
                |_| PlaylistsMessage::SetOpenMenu(None),
            ))
        } else if let Some(handles) = collage_handles.filter(|v| !v.is_empty()) {
            // Render 3x3 collage grid (2+ albums)
            Some(collage_artwork_panel_with_pill::<PlaylistsMessage>(
                handles,
                pill_content,
            ))
        } else {
            // album_count > 1 but collage NOT loaded yet - show placeholder
            Some(single_artwork_panel_with_pill::<PlaylistsMessage>(
                None,
                pill_content,
                None,
                None,
                false,
                None,
                |_| PlaylistsMessage::SetOpenMenu(None),
            ))
        };

        crate::widgets::base_slot_list_layout::base_slot_list_layout_with_handle(
            &layout_config,
            header,
            slot_list_content,
            artwork_content,
            Some(PlaylistsMessage::ArtworkColumnDrag),
        )
    }

    /// Render a parent playlist row in the slot list
    fn render_playlist_row<'a>(
        &self,
        playlist: &PlaylistUIViewData,
        ctx: &crate::widgets::slot_list::SlotListRowContext,
        playlist_artwork: &'a HashMap<String, image::Handle>,
        stable_viewport: bool,
        open_menu: Option<&'a crate::app_message::OpenMenu>,
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

        let m = ctx.metrics;
        let artwork_size = m.artwork_size;
        let title_size = m.title_size;
        let metadata_size = m.metadata_size;
        let index_size = m.metadata_size;

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
        let show_song_count_col =
            playlists_song_count_visible(sort_mode, self.column_visibility.songcount);
        let show_duration_col =
            playlists_duration_visible(sort_mode, self.column_visibility.duration);
        let show_updated_at =
            playlists_updated_at_visible(sort_mode, self.column_visibility.updatedat);

        // Song-count text is only consumed by the dedicated column below
        // (when toggled on or auto-shown by sort). The subtitle no longer
        // falls back to it — toggling a column off means hide, full stop.
        let count_text = if playlist.song_count == 1 {
            "1 song".to_string()
        } else {
            format!("{} songs", playlist.song_count)
        };
        let subtitle = String::new();

        // Extra columns reduce the name portion to make room
        let extra_cols =
            show_song_count_col as u16 + show_duration_col as u16 + show_updated_at as u16;
        let name_portion = 55 - extra_cols * 10;

        // Layout: [Index?] [Artwork?] [Name+subtitle] [SongCount?] [Duration?] [UpdatedAt?]
        use crate::widgets::slot_list::{slot_list_artwork_column, slot_list_metadata_column};

        let mut columns: Vec<Element<'a, PlaylistsMessage>> = Vec::new();
        if self.column_visibility.index {
            columns.push(slot_list_index_column(
                ctx.item_index,
                index_size,
                style,
                ctx.opacity,
            ));
        }
        if self.column_visibility.thumbnail {
            columns.push(slot_list_artwork_column(
                playlist_artwork.get(&playlist.id),
                artwork_size,
                ctx.is_center,
                false,
                ctx.opacity,
            ));
        }
        columns.push({
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
        });

        // Visibility glyph slot — always pushed so the row's widget tree
        // shape stays identical between public/private states. Public renders
        // a zero-width Space; private renders a lock SVG in muted fg3 with a
        // tooltip explaining why the icon is there.
        columns.push(if playlist.public {
            iced::widget::Space::new()
                .width(Length::Fixed(0.0))
                .height(Length::Fixed(14.0))
                .into()
        } else {
            let lock_icon = crate::embedded_svg::svg_widget("assets/icons/lock.svg")
                .width(Length::Fixed(14.0))
                .height(Length::Fixed(14.0))
                .style(|_theme, _status| iced::widget::svg::Style {
                    color: Some(crate::theme::fg3()),
                });
            iced::widget::tooltip(
                lock_icon,
                iced::widget::container(
                    iced::widget::text("Private playlist")
                        .size(11.0)
                        .font(crate::theme::ui_font()),
                )
                .padding(4),
                iced::widget::tooltip::Position::Top,
            )
            .gap(4)
            .style(crate::theme::container_tooltip)
            .into()
        });

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

        use crate::widgets::context_menu::{context_menu, open_state_for};
        let item_idx = ctx.item_index;
        let cm_id = crate::app_message::ContextMenuId::LibraryRow {
            view: crate::View::Playlists,
            item_index: item_idx,
        };
        let (cm_open, cm_position) = open_state_for(open_menu, &cm_id);
        context_menu(
            slot_button,
            playlist_context_entries(),
            move |entry, length| {
                playlist_entry_view(entry, length, |e| {
                    PlaylistsMessage::PlaylistContextAction(item_idx, e)
                })
            },
            cm_open,
            cm_position,
            move |position| match position {
                Some(p) => {
                    PlaylistsMessage::SetOpenMenu(Some(crate::app_message::OpenMenu::Context {
                        id: cm_id.clone(),
                        position: p,
                    }))
                }
                None => PlaylistsMessage::SetOpenMenu(None),
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
            song.artist_id
                .as_ref()
                .map(|id| PlaylistsMessage::NavigateAndExpandArtist(id.clone())),
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
