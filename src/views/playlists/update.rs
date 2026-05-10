//! Playlists view — `impl PlaylistsPage { fn update }`.
//!
//! Handler for `PlaylistsMessage`. View rendering lives in `view.rs`;
//! types live in `mod.rs`.

use iced::Task;
use nokkvi_data::{backend::playlists::PlaylistUIViewData, types::ItemKind};

use super::{
    super::expansion::SlotListEntry, PlaylistContextEntry, PlaylistsAction, PlaylistsMessage,
    PlaylistsPage,
};

impl PlaylistsPage {
    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: PlaylistsMessage,
        total_items: usize,
        playlists: &[PlaylistUIViewData],
    ) -> (Task<PlaylistsMessage>, PlaylistsAction) {
        match super::super::impl_expansion_update!(
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
                    // Flattened (parents + expansion children) index space —
                    // `total_items` from the dispatcher is the base count.
                    let flattened = self.expansion.flattened_len(playlists);
                    self.common.handle_selection_toggle(offset, flattened);
                    (Task::none(), PlaylistsAction::None)
                }
                PlaylistsMessage::SlotListSelectAllToggle => {
                    let flattened = self.expansion.flattened_len(playlists);
                    self.common.handle_select_all_toggle(flattened);
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

                    let payload = super::super::expansion::build_batch_payload(
                        target_indices,
                        |i| match self.expansion.get_entry_at(i, playlists, |p| &p.id) {
                            Some(SlotListEntry::Parent(playlist)) => {
                                Some(BatchItem::Playlist(playlist.id.clone()))
                            }
                            Some(SlotListEntry::Child(song, _)) => {
                                let item: nokkvi_data::types::song::Song = song.clone().into();
                                Some(BatchItem::Song(Box::new(item)))
                            }
                            None => None,
                        },
                    );

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
                                    ItemKind::Song,
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
                // Routed up to root in `handle_playlists` before this match
                // runs; arm exists only for exhaustiveness.
                PlaylistsMessage::SetOpenMenu(_) => (Task::none(), PlaylistsAction::None),
                PlaylistsMessage::Roulette => (Task::none(), PlaylistsAction::None),
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
                        let payload =
                            super::super::expansion::build_batch_payload(target_indices, |i| {
                                match self.expansion.get_entry_at(i, playlists, |p| &p.id) {
                                    Some(SlotListEntry::Parent(playlist)) => {
                                        Some(BatchItem::Playlist(playlist.id.clone()))
                                    }
                                    Some(SlotListEntry::Child(song, _)) => {
                                        let item: nokkvi_data::types::song::Song =
                                            song.clone().into();
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
                        let payload =
                            super::super::expansion::build_batch_payload(target_indices, |i| {
                                match self.expansion.get_entry_at(i, playlists, |p| &p.id) {
                                    Some(SlotListEntry::Parent(playlist)) => {
                                        Some(BatchItem::Playlist(playlist.id.clone()))
                                    }
                                    Some(SlotListEntry::Child(song, _)) => {
                                        let item: nokkvi_data::types::song::Song =
                                            song.clone().into();
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
}
