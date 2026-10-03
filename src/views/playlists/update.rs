//! Playlists view — `impl PlaylistsPage { fn update }`.
//!
//! Handler for `PlaylistsMessage`. View rendering lives in `view.rs`;
//! types live in `mod.rs`.

use iced::Task;
use nokkvi_data::{
    backend::{playlists::PlaylistUIViewData, songs::SongUIViewData},
    types::{ItemKind, batch::BatchItem, info_modal::InfoModalItem, trawl::TrawlSeed},
};

use super::{
    super::expansion::SlotListEntry, PlaylistContextEntry, PlaylistsAction, PlaylistsMessage,
    PlaylistsPage,
};
use crate::widgets::context_menu::LibraryContextEntry;

type PlaylistsRow<'a> = SlotListEntry<&'a PlaylistUIViewData, &'a SongUIViewData>;

impl PlaylistsPage {
    /// What a row stands for in a play / queue / playlist batch.
    fn batch_item(row: PlaylistsRow<'_>) -> BatchItem {
        match row {
            SlotListEntry::Parent(playlist) => BatchItem::Playlist(playlist.id.clone()),
            SlotListEntry::Child(song, _) => BatchItem::Song(Box::new(song.clone().into())),
        }
    }

    /// The Trawl seed a row adds to the crate.
    fn trawl_seed(row: PlaylistsRow<'_>) -> TrawlSeed {
        match row {
            SlotListEntry::Parent(playlist) => TrawlSeed::from_playlist(
                playlist.id.clone(),
                playlist.name.clone(),
                playlist.song_count,
            ),
            SlotListEntry::Child(song, _) => TrawlSeed::from_song(song.clone().into()),
        }
    }

    /// The rows a batch menu entry acts on: the selection when the clicked
    /// row is part of it, else the clicked row alone (clears the selection).
    fn menu_targets<'a>(
        &'a mut self,
        clicked_idx: usize,
        playlists: &'a [PlaylistUIViewData],
    ) -> impl Iterator<Item = PlaylistsRow<'a>> + 'a {
        let targets = self.common.get_batch_target_indices(clicked_idx);
        self.expansion.rows_at(targets, playlists, |p| &p.id)
    }

    /// Resolve a click in an expanded track row's menu (the shared library
    /// entries). Batch entries act on the selection when the clicked row is
    /// part of it; the rest act on the clicked row alone.
    fn track_menu_action(
        &mut self,
        clicked_idx: usize,
        entry: LibraryContextEntry,
        playlists: &[PlaylistUIViewData],
    ) -> PlaylistsAction {
        use LibraryContextEntry as Entry;
        let row = self
            .expansion
            .get_entry_at(clicked_idx, playlists, |p| &p.id);
        match (entry, row) {
            (Entry::ShufflePlay, _) => PlaylistsAction::PlayBatch(
                self.menu_targets(clicked_idx, playlists)
                    .map(Self::batch_item)
                    .collect(),
                true,
            ),
            (Entry::AddToQueue, _) => PlaylistsAction::AddBatchToQueue(
                self.menu_targets(clicked_idx, playlists)
                    .map(Self::batch_item)
                    .collect(),
            ),
            (Entry::AddToPlaylist, _) => PlaylistsAction::AddBatchToPlaylist(
                self.menu_targets(clicked_idx, playlists)
                    .map(Self::batch_item)
                    .collect(),
            ),
            (Entry::AddToMix, _) => PlaylistsAction::AddBatchToMix(
                self.menu_targets(clicked_idx, playlists)
                    .map(Self::trawl_seed)
                    .collect(),
            ),
            (Entry::GetInfo, Some(SlotListEntry::Child(song, _))) => {
                PlaylistsAction::ShowInfo(Box::new(InfoModalItem::from_song_view_data(song)))
            }
            // Single-row and ordinal-addressed (never id-addressed: a
            // playlist can hold the same song twice). Position is 1-based on
            // the wire. Smart parents never offer the entry.
            (Entry::RemoveFromPlaylist, Some(SlotListEntry::Child(song, playlist_id))) => {
                match self
                    .expansion
                    .child_ordinal_at(clicked_idx, playlists, |p| &p.id)
                {
                    Some(ordinal) => PlaylistsAction::RemoveTrackFromPlaylist {
                        playlist_id,
                        song_id: song.id.clone(),
                        position: ordinal as u32 + 1,
                    },
                    None => PlaylistsAction::None,
                }
            }
            // Playlist rows use `playlist_menu_action`; track rows here offer
            // neither a folder nor Find Similar / Top Songs, and the rest are
            // other views' entries.
            (Entry::GetInfo | Entry::RemoveFromPlaylist, Some(SlotListEntry::Parent(_)) | None)
            | (
                Entry::ShowInFolder
                | Entry::FindSimilar
                | Entry::TopSongs
                | Entry::Separator
                | Entry::ReplaceQueueWithAllFound
                | Entry::AddAllFoundToQueue
                | Entry::AddAllFoundToPlaylist,
                _,
            ) => PlaylistsAction::None,
        }
    }

    /// Resolve a click in a playlist row's menu. Add to Queue / Add to Mix
    /// act on the selection when the clicked row is part of it; the rest act
    /// on the clicked playlist alone.
    fn playlist_menu_action(
        &mut self,
        clicked_idx: usize,
        entry: PlaylistContextEntry,
        playlists: &[PlaylistUIViewData],
    ) -> PlaylistsAction {
        use LibraryContextEntry as Entry;
        use PlaylistContextEntry as Pl;
        let row = self
            .expansion
            .get_entry_at(clicked_idx, playlists, |p| &p.id);
        match (entry, row) {
            (Pl::Library(Entry::AddToQueue), _) => PlaylistsAction::AddBatchToQueue(
                self.menu_targets(clicked_idx, playlists)
                    .map(Self::batch_item)
                    .collect(),
            ),
            (Pl::Library(Entry::AddToMix), _) => PlaylistsAction::AddBatchToMix(
                self.menu_targets(clicked_idx, playlists)
                    .map(Self::trawl_seed)
                    .collect(),
            ),
            (Pl::Library(Entry::GetInfo), Some(SlotListEntry::Parent(playlist))) => {
                PlaylistsAction::ShowInfo(Box::new(InfoModalItem::from_playlist_view_data(
                    playlist,
                )))
            }
            (Pl::Delete, Some(SlotListEntry::Parent(playlist))) => {
                PlaylistsAction::DeletePlaylist(playlist.id.clone())
            }
            (Pl::Rename, Some(SlotListEntry::Parent(playlist))) => {
                PlaylistsAction::RenamePlaylist(playlist.id.clone())
            }
            (Pl::EditPlaylist, Some(SlotListEntry::Parent(playlist))) => {
                PlaylistsAction::EditPlaylist(
                    playlist.id.clone(),
                    playlist.name.clone(),
                    playlist.comment.clone(),
                    playlist.public,
                )
            }
            (Pl::EditRules, Some(SlotListEntry::Parent(playlist))) => {
                PlaylistsAction::EditRules(playlist.id.clone())
            }
            (Pl::SetAsDefault, Some(SlotListEntry::Parent(playlist))) => {
                PlaylistsAction::SetAsDefaultPlaylist(playlist.id.clone(), playlist.name.clone())
            }
            (Pl::SetCustomArtwork, Some(SlotListEntry::Parent(playlist))) => {
                PlaylistsAction::SetCustomArtwork(playlist.id.clone(), playlist.name.clone())
            }
            (Pl::ResetArtwork, Some(SlotListEntry::Parent(playlist))) => {
                PlaylistsAction::ResetCustomArtwork(playlist.id.clone(), playlist.name.clone())
            }
            // Only playlist rows carry this menu, and it offers no other
            // library entries.
            (
                Pl::Library(Entry::GetInfo)
                | Pl::Delete
                | Pl::Rename
                | Pl::EditPlaylist
                | Pl::EditRules
                | Pl::SetAsDefault
                | Pl::SetCustomArtwork
                | Pl::ResetArtwork,
                Some(SlotListEntry::Child(..)) | None,
            )
            | (
                Pl::Library(
                    Entry::ShufflePlay
                    | Entry::AddToPlaylist
                    | Entry::Separator
                    | Entry::ShowInFolder
                    | Entry::FindSimilar
                    | Entry::TopSongs
                    | Entry::RemoveFromPlaylist
                    | Entry::ReplaceQueueWithAllFound
                    | Entry::AddAllFoundToQueue
                    | Entry::AddAllFoundToPlaylist,
                )
                | Pl::Separator,
                _,
            ) => PlaylistsAction::None,
        }
    }

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
            slot_list_wrap: PlaylistsMessage::SlotList,
            action_none: PlaylistsAction::None,
        ) {
            Ok(result) => result,
            Err(msg) => match msg {
                PlaylistsMessage::SlotList(msg) => {
                    use crate::widgets::SlotListPageMessage;
                    match msg {
                        SlotListPageMessage::NavigateUp => {
                            let center = self
                                .expansion
                                .handle_navigate_up(playlists, &mut self.common);
                            match center {
                                Some(idx) => {
                                    (Task::none(), PlaylistsAction::LoadArtwork(idx.to_string()))
                                }
                                None => (Task::none(), PlaylistsAction::None),
                            }
                        }
                        SlotListPageMessage::NavigateDown => {
                            let center = self
                                .expansion
                                .handle_navigate_down(playlists, &mut self.common);
                            match center {
                                Some(idx) => {
                                    (Task::none(), PlaylistsAction::LoadArtwork(idx.to_string()))
                                }
                                None => (Task::none(), PlaylistsAction::None),
                            }
                        }
                        SlotListPageMessage::SetOffset(offset, modifiers) => {
                            let center = self.expansion.handle_select_offset(
                                offset,
                                modifiers,
                                playlists,
                                &mut self.common,
                            );
                            match center {
                                Some(idx) => {
                                    (Task::none(), PlaylistsAction::LoadArtwork(idx.to_string()))
                                }
                                None => (Task::none(), PlaylistsAction::None),
                            }
                        }
                        SlotListPageMessage::ScrollSeek(offset) => {
                            self.expansion
                                .handle_set_offset(offset, playlists, &mut self.common);
                            (Task::none(), PlaylistsAction::None)
                        }
                        SlotListPageMessage::ClickPlay(offset) => {
                            self.expansion
                                .handle_set_offset(offset, playlists, &mut self.common);
                            self.update(
                                PlaylistsMessage::SlotList(SlotListPageMessage::ActivateCenter(
                                    false,
                                )),
                                total_items,
                                playlists,
                            )
                        }
                        SlotListPageMessage::SelectionToggle(offset) => {
                            // Flattened (parents + expansion children) index space —
                            // `total_items` from the dispatcher is the base count.
                            let flattened = self.expansion.flattened_len(playlists);
                            self.common.handle_selection_toggle(offset, flattened);
                            (Task::none(), PlaylistsAction::None)
                        }
                        SlotListPageMessage::SelectAllToggle => {
                            let flattened = self.expansion.flattened_len(playlists);
                            self.common.handle_select_all_toggle(flattened);
                            (Task::none(), PlaylistsAction::None)
                        }
                        SlotListPageMessage::ActivateCenter(force) => {
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
                                                force,
                                            ),
                                        )
                                    }
                                    Some(SlotListEntry::Parent(playlist)) => (
                                        Task::none(),
                                        PlaylistsAction::PlayPlaylist(playlist.id.clone(), force),
                                    ),
                                    None => (Task::none(), PlaylistsAction::None),
                                }
                            } else {
                                (Task::none(), PlaylistsAction::None)
                            }
                        }
                        SlotListPageMessage::AddCenterToQueue => {
                            use nokkvi_data::types::batch::BatchItem;
                            let total = self.expansion.flattened_len(playlists);

                            let target_indices = self.common.get_queue_target_indices(total);

                            if target_indices.is_empty() {
                                return (Task::none(), PlaylistsAction::None);
                            }

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

                            (Task::none(), PlaylistsAction::AddBatchToQueue(payload))
                        }
                        SlotListPageMessage::RefreshViewData => {
                            (Task::none(), PlaylistsAction::RefreshViewData)
                        }
                        // Playlists does not emit CenterOnPlaying; exhaustiveness arm only.
                        SlotListPageMessage::CenterOnPlaying => {
                            (Task::none(), PlaylistsAction::None)
                        }
                        // Sort/search/hover exhaustiveness arms — all handled by
                        // impl_expansion_update! above.
                        SlotListPageMessage::SearchQueryChanged(_)
                        | SlotListPageMessage::SearchFocused(_)
                        | SlotListPageMessage::SortModeSelected(_)
                        | SlotListPageMessage::ToggleSortOrder
                        | SlotListPageMessage::HoverEnterSlot(_)
                        | SlotListPageMessage::HoverExitSlot(_)
                        | SlotListPageMessage::ToolbarHoverEnter
                        | SlotListPageMessage::ToolbarHoverExit
                        | SlotListPageMessage::ToolbarDropdownToggled(_) => {
                            (Task::none(), PlaylistsAction::None)
                        }
                    }
                }
                PlaylistsMessage::FocusAndExpand(idx) => {
                    self.common.slot_list.clear_selection_indices_only();
                    let (t1, _) = self.update(
                        PlaylistsMessage::SlotList(crate::widgets::SlotListPageMessage::SetOffset(
                            idx,
                            iced::keyboard::Modifiers::default(),
                        )),
                        total_items,
                        playlists,
                    );
                    let (t2, action) =
                        self.update(PlaylistsMessage::ExpandCenter, total_items, playlists);
                    (Task::batch(vec![t1, t2]), action)
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
                PlaylistsMessage::NavigateAndFilter(view, filter) => (
                    Task::none(),
                    PlaylistsAction::NavigateAndFilter(view, filter),
                ),
                PlaylistsMessage::NavigateAndExpandArtist(artist_id) => (
                    Task::none(),
                    PlaylistsAction::NavigateAndExpandArtist(artist_id),
                ),

                PlaylistsMessage::SetCustomArtwork(id, name) => {
                    (Task::none(), PlaylistsAction::SetCustomArtwork(id, name))
                }
                PlaylistsMessage::ResetCustomArtwork(id, name) => {
                    (Task::none(), PlaylistsAction::ResetCustomArtwork(id, name))
                }
                PlaylistsMessage::OpenDefaultPlaylistPicker => {
                    (Task::none(), PlaylistsAction::OpenDefaultPlaylistPicker)
                }
                PlaylistsMessage::NewPlaylistInEditor => {
                    (Task::none(), PlaylistsAction::NewPlaylistInEditor)
                }
                PlaylistsMessage::ToggleColumnVisible(col) => {
                    let new_value = self.column_visibility.toggle(col);
                    (
                        Task::none(),
                        PlaylistsAction::ColumnVisibilityChanged(col, new_value),
                    )
                }

                PlaylistsMessage::TrackRemovalSettled {
                    playlist_id,
                    removed,
                } => (
                    Task::none(),
                    PlaylistsAction::TrackRemovalSettled {
                        playlist_id,
                        removed,
                    },
                ),
                PlaylistsMessage::NewSmartPlaylist => {
                    (Task::none(), PlaylistsAction::NewSmartPlaylist)
                }
                PlaylistsMessage::ImportNsp => (Task::none(), PlaylistsAction::ImportNsp),
                PlaylistsMessage::RetryCapsFetch => (Task::none(), PlaylistsAction::RetryCapsFetch),
                PlaylistsMessage::ContextMenuAction(clicked_idx, entry) => (
                    Task::none(),
                    self.track_menu_action(clicked_idx, entry, playlists),
                ),
                PlaylistsMessage::PlaylistContextAction(clicked_idx, entry) => (
                    Task::none(),
                    self.playlist_menu_action(clicked_idx, entry, playlists),
                ),
                // Common arms already handled by macro above
                _ => (Task::none(), PlaylistsAction::None),
            },
        }
    }
}
