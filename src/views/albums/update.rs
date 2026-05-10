//! Albums view — `impl AlbumsPage { fn update }`.
//!
//! Handler for `AlbumsMessage`. View rendering lives in `view.rs`;
//! types live in `mod.rs`.

use iced::Task;
use nokkvi_data::{backend::albums::AlbumUIViewData, types::ItemKind};

use super::{super::expansion::SlotListEntry, AlbumsAction, AlbumsMessage, AlbumsPage};

impl AlbumsPage {
    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: AlbumsMessage,
        total_items: usize,
        albums: &[AlbumUIViewData],
    ) -> (Task<AlbumsMessage>, AlbumsAction) {
        match super::super::impl_expansion_update!(
            self, message, albums, total_items,
            id_fn: |a| &a.id,
            expand_center: AlbumsMessage::ExpandCenter => AlbumsAction::ExpandAlbum,
            collapse: AlbumsMessage::CollapseExpansion,
            children_loaded: AlbumsMessage::TracksLoaded,
            sort_selected: AlbumsMessage::SortModeSelected => AlbumsAction::SortModeChanged,
            toggle_sort: AlbumsMessage::ToggleSortOrder => AlbumsAction::SortOrderChanged,
            search_changed: AlbumsMessage::SearchQueryChanged => AlbumsAction::SearchChanged,
            search_focused: AlbumsMessage::SearchFocused,
            action_none: AlbumsAction::None,
        ) {
            Ok(result) => result,
            Err(msg) => match msg {
                AlbumsMessage::SlotListNavigateUp => {
                    let center = self.expansion.handle_navigate_up(albums, &mut self.common);
                    match center {
                        Some(idx) => (
                            Task::none(),
                            AlbumsAction::LoadLargeArtwork(idx.to_string()),
                        ),
                        None => (Task::none(), AlbumsAction::None),
                    }
                }
                AlbumsMessage::SlotListNavigateDown => {
                    let center = self
                        .expansion
                        .handle_navigate_down(albums, &mut self.common);
                    match center {
                        Some(idx) => (
                            Task::none(),
                            AlbumsAction::LoadLargeArtwork(idx.to_string()),
                        ),
                        None => (Task::none(), AlbumsAction::None),
                    }
                }
                AlbumsMessage::SlotListSetOffset(offset, modifiers) => {
                    let center = self.expansion.handle_select_offset(
                        offset,
                        modifiers,
                        albums,
                        &mut self.common,
                    );
                    match center {
                        Some(idx) => (
                            Task::none(),
                            AlbumsAction::LoadLargeArtwork(idx.to_string()),
                        ),
                        None => (Task::none(), AlbumsAction::None),
                    }
                }
                AlbumsMessage::FocusAndExpand(offset) => {
                    let center = self.expansion.handle_select_offset(
                        offset,
                        Default::default(),
                        albums,
                        &mut self.common,
                    );
                    if let Some(idx) = center {
                        // Now expand it
                        if let Some(parent_id) =
                            self.expansion
                                .handle_expand_center(albums, |a| &a.id, &mut self.common)
                        {
                            (Task::none(), AlbumsAction::ExpandAlbum(parent_id))
                        } else {
                            (
                                Task::none(),
                                AlbumsAction::LoadLargeArtwork(idx.to_string()),
                            )
                        }
                    } else {
                        (Task::none(), AlbumsAction::None)
                    }
                }
                AlbumsMessage::SlotListScrollSeek(offset) => {
                    self.expansion
                        .handle_set_offset(offset, albums, &mut self.common);
                    (Task::none(), AlbumsAction::None)
                }
                AlbumsMessage::SlotListClickPlay(offset) => {
                    // Set offset then activate (play without focusing)
                    self.expansion
                        .handle_set_offset(offset, albums, &mut self.common);
                    self.update(AlbumsMessage::SlotListActivateCenter, total_items, albums)
                }
                AlbumsMessage::SlotListSelectionToggle(offset) => {
                    // Slot list indices are flattened (parents + expansion
                    // children); `total_items` from the dispatcher is the
                    // base buffer length. Use the flattened length so the
                    // toggle's bounds check matches what the user sees.
                    let flattened = self.expansion.flattened_len(albums);
                    self.common.handle_selection_toggle(offset, flattened);
                    (Task::none(), AlbumsAction::None)
                }
                AlbumsMessage::SlotListSelectAllToggle => {
                    let flattened = self.expansion.flattened_len(albums);
                    self.common.handle_select_all_toggle(flattened);
                    (Task::none(), AlbumsAction::None)
                }
                AlbumsMessage::SlotListActivateCenter => {
                    let total = self.expansion.flattened_len(albums);
                    let center_idx = self.common.get_center_item_index(total);
                    let target_indices = self
                        .common
                        .slot_list
                        .selected_indices
                        .iter()
                        .copied()
                        .collect::<Vec<_>>();

                    if !target_indices.is_empty() {
                        use nokkvi_data::types::batch::{BatchItem, BatchPayload};
                        let payload = target_indices
                            .into_iter()
                            .filter_map(|i| {
                                match self.expansion.get_entry_at(i, albums, |a| &a.id) {
                                    Some(SlotListEntry::Parent(album)) => {
                                        Some(BatchItem::Album(album.id.clone()))
                                    }
                                    Some(SlotListEntry::Child(song, _)) => {
                                        let item: nokkvi_data::types::song::Song =
                                            song.clone().into();
                                        Some(BatchItem::Song(Box::new(item)))
                                    }
                                    None => None,
                                }
                            })
                            .fold(BatchPayload::new(), |p, item| p.with_item(item));
                        return (Task::none(), AlbumsAction::PlayBatch(payload));
                    }

                    if let Some(center_idx) = center_idx {
                        self.common.slot_list.flash_center();
                        match self.expansion.get_entry_at(center_idx, albums, |a| &a.id) {
                            Some(SlotListEntry::Child(_song, parent_album_id)) => {
                                let track_index =
                                    self.expansion
                                        .count_children_before(center_idx, albums, |a| &a.id);
                                (
                                    Task::none(),
                                    AlbumsAction::PlayAlbumFromTrack(parent_album_id, track_index),
                                )
                            }
                            Some(SlotListEntry::Parent(_)) => (
                                Task::none(),
                                AlbumsAction::PlayAlbum(center_idx.to_string()),
                            ),
                            None => (Task::none(), AlbumsAction::None),
                        }
                    } else {
                        (Task::none(), AlbumsAction::None)
                    }
                }
                AlbumsMessage::AddCenterToQueue => {
                    use nokkvi_data::types::batch::BatchItem;

                    let total = self.expansion.flattened_len(albums);
                    let target_indices = self.common.get_queue_target_indices(total);

                    if target_indices.is_empty() {
                        return (Task::none(), AlbumsAction::None);
                    }

                    let payload = super::super::expansion::build_batch_payload(
                        target_indices,
                        |i| match self.expansion.get_entry_at(i, albums, |a| &a.id) {
                            Some(SlotListEntry::Parent(album)) => {
                                Some(BatchItem::Album(album.id.clone()))
                            }
                            Some(SlotListEntry::Child(song, _)) => {
                                let item: nokkvi_data::types::song::Song = song.clone().into();
                                Some(BatchItem::Song(Box::new(item)))
                            }
                            None => None,
                        },
                    );

                    (Task::none(), AlbumsAction::AddBatchToQueue(payload))
                }
                // Data loading messages (handled at root level, no action needed here)
                AlbumsMessage::ArtworkLoaded(_, _) => (Task::none(), AlbumsAction::None),
                AlbumsMessage::LargeArtworkLoaded(_, _) => (Task::none(), AlbumsAction::None),
                // Routed up to root in `handle_albums` before this match runs;
                // arm exists only for exhaustiveness.
                AlbumsMessage::SetOpenMenu(_) => (Task::none(), AlbumsAction::None),
                AlbumsMessage::Roulette => (Task::none(), AlbumsAction::None),
                AlbumsMessage::RefreshViewData => (Task::none(), AlbumsAction::RefreshViewData),
                AlbumsMessage::RefreshArtwork(album_id) => {
                    (Task::none(), AlbumsAction::RefreshArtwork(album_id))
                }
                AlbumsMessage::ClickSetRating(item_index, rating) => {
                    if let Some(entry) = self.expansion.get_entry_at(item_index, albums, |a| &a.id)
                    {
                        use nokkvi_data::utils::formatters::compute_rating_toggle;
                        match entry {
                            SlotListEntry::Child(song, _) => {
                                let current = song.rating.unwrap_or(0) as usize;
                                let new_rating = compute_rating_toggle(current, rating);
                                (
                                    Task::none(),
                                    AlbumsAction::SetRating(
                                        song.id.clone(),
                                        ItemKind::Song,
                                        new_rating,
                                    ),
                                )
                            }
                            SlotListEntry::Parent(album) => {
                                let current = album.rating.unwrap_or(0) as usize;
                                let new_rating = compute_rating_toggle(current, rating);
                                (
                                    Task::none(),
                                    AlbumsAction::SetRating(
                                        album.id.clone(),
                                        ItemKind::Album,
                                        new_rating,
                                    ),
                                )
                            }
                        }
                    } else {
                        (Task::none(), AlbumsAction::None)
                    }
                }
                AlbumsMessage::ClickToggleStar(item_index) => {
                    if let Some(entry) = self.expansion.get_entry_at(item_index, albums, |a| &a.id)
                    {
                        match entry {
                            SlotListEntry::Child(song, _) => (
                                Task::none(),
                                AlbumsAction::ToggleStar(
                                    song.id.clone(),
                                    ItemKind::Song,
                                    !song.is_starred,
                                ),
                            ),
                            SlotListEntry::Parent(album) => (
                                Task::none(),
                                AlbumsAction::ToggleStar(
                                    album.id.clone(),
                                    ItemKind::Album,
                                    !album.is_starred,
                                ),
                            ),
                        }
                    } else {
                        (Task::none(), AlbumsAction::None)
                    }
                }
                AlbumsMessage::ContextMenuAction(clicked_idx, entry) => {
                    use nokkvi_data::types::batch::BatchItem;

                    use crate::widgets::context_menu::LibraryContextEntry;

                    match entry {
                        LibraryContextEntry::AddToQueue | LibraryContextEntry::AddToPlaylist => {
                            let target_indices = self.common.get_batch_target_indices(clicked_idx);
                            let payload =
                                super::super::expansion::build_batch_payload(target_indices, |i| {
                                    match self.expansion.get_entry_at(i, albums, |a| &a.id) {
                                        Some(SlotListEntry::Parent(album)) => {
                                            Some(BatchItem::Album(album.id.clone()))
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
                                    (Task::none(), AlbumsAction::AddBatchToQueue(payload))
                                }
                                LibraryContextEntry::AddToPlaylist => {
                                    (Task::none(), AlbumsAction::AddBatchToPlaylist(payload))
                                }
                                _ => unreachable!(),
                            }
                        }
                        // Non-batched actions (apply only to the clicked item)
                        _ => match self.expansion.get_entry_at(clicked_idx, albums, |a| &a.id) {
                            Some(SlotListEntry::Parent(album)) => match entry {
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
                                            .expansion
                                            .children
                                            .first()
                                            .map(|s| s.path.clone()),
                                    };
                                    (Task::none(), AlbumsAction::ShowInfo(Box::new(item)))
                                }
                                LibraryContextEntry::ShowInFolder => {
                                    (Task::none(), AlbumsAction::ShowInFolder(album.id.clone()))
                                }
                                LibraryContextEntry::FindSimilar => (
                                    Task::none(),
                                    AlbumsAction::FindSimilar(
                                        album.artist.clone(),
                                        format!("Similar to: {}", album.name),
                                    ),
                                ),
                                LibraryContextEntry::Separator => {
                                    (Task::none(), AlbumsAction::None)
                                }
                                _ => (Task::none(), AlbumsAction::None),
                            },
                            Some(SlotListEntry::Child(song, _)) => match entry {
                                LibraryContextEntry::GetInfo => {
                                    use nokkvi_data::types::info_modal::InfoModalItem;
                                    let item = InfoModalItem::from_song_view_data(song);
                                    (Task::none(), AlbumsAction::ShowInfo(Box::new(item)))
                                }
                                LibraryContextEntry::ShowInFolder => (
                                    Task::none(),
                                    AlbumsAction::ShowSongInFolder(song.path.clone()),
                                ),
                                LibraryContextEntry::FindSimilar => (
                                    Task::none(),
                                    AlbumsAction::FindSimilar(
                                        song.id.clone(),
                                        format!("Similar to: {}", song.title),
                                    ),
                                ),
                                LibraryContextEntry::Separator => {
                                    (Task::none(), AlbumsAction::None)
                                }
                                _ => (Task::none(), AlbumsAction::None),
                            },
                            None => (Task::none(), AlbumsAction::None),
                        },
                    }
                }
                // Common arms already handled by macro above
                AlbumsMessage::CenterOnPlaying => (Task::none(), AlbumsAction::CenterOnPlaying),
                AlbumsMessage::NavigateAndFilter(view, filter) => {
                    (Task::none(), AlbumsAction::NavigateAndFilter(view, filter))
                }
                AlbumsMessage::NavigateAndExpandArtist(artist_id) => (
                    Task::none(),
                    AlbumsAction::NavigateAndExpandArtist(artist_id),
                ),
                AlbumsMessage::NavigateAndExpandGenre(genre_id) => {
                    (Task::none(), AlbumsAction::NavigateAndExpandGenre(genre_id))
                }
                AlbumsMessage::ToggleColumnVisible(col) => {
                    let new_value = !self.column_visibility.get(col);
                    self.column_visibility.set(col, new_value);
                    (
                        Task::none(),
                        AlbumsAction::ColumnVisibilityChanged(col, new_value),
                    )
                }
                _ => (Task::none(), AlbumsAction::None),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::albums::AlbumsColumn;

    #[test]
    fn center_on_playing_translates_to_action() {
        let mut page = AlbumsPage::new();
        let empty_albums: Vec<AlbumUIViewData> = vec![];
        let (_, action) = page.update(AlbumsMessage::CenterOnPlaying, 0, &empty_albums);

        assert!(matches!(action, AlbumsAction::CenterOnPlaying));
    }

    #[test]
    fn albums_toggle_column_visible_flips_state_and_emits_action() {
        let mut page = AlbumsPage::default();
        let empty: Vec<AlbumUIViewData> = vec![];
        let (_t, action) = page.update(
            AlbumsMessage::ToggleColumnVisible(AlbumsColumn::Stars),
            0,
            &empty,
        );
        assert!(page.column_visibility.stars);
        assert!(matches!(
            action,
            AlbumsAction::ColumnVisibilityChanged(AlbumsColumn::Stars, true)
        ));
    }
}
