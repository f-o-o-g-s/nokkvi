//! Albums view — `impl AlbumsPage { fn update }`.
//!
//! Handler for `AlbumsMessage`. View rendering lives in `view.rs`;
//! types live in `mod.rs`.

use iced::Task;
use nokkvi_data::{
    backend::{albums::AlbumUIViewData, songs::SongUIViewData},
    types::{ItemKind, batch::BatchItem, info_modal::InfoModalItem, trawl::TrawlSeed},
};

use super::{super::expansion::SlotListEntry, AlbumsAction, AlbumsMessage, AlbumsPage};
use crate::widgets::context_menu::LibraryContextEntry;

type AlbumsRow<'a> = SlotListEntry<&'a AlbumUIViewData, &'a SongUIViewData>;

impl AlbumsPage {
    /// What a row stands for in a play / queue / playlist batch.
    fn batch_item(row: AlbumsRow<'_>) -> BatchItem {
        match row {
            SlotListEntry::Parent(album) => BatchItem::Album(album.id.clone()),
            SlotListEntry::Child(song, _) => BatchItem::Song(Box::new(song.clone().into())),
        }
    }

    /// The Trawl seed a row adds to the crate.
    fn trawl_seed(row: AlbumsRow<'_>) -> TrawlSeed {
        match row {
            SlotListEntry::Parent(album) => {
                TrawlSeed::from_album(album.id.clone(), album.name.clone(), album.artist.clone())
            }
            SlotListEntry::Child(song, _) => TrawlSeed::from_song(song.clone().into()),
        }
    }

    /// The rows a batch menu entry acts on: the selection when the clicked
    /// row is part of it, else the clicked row alone (clears the selection).
    fn menu_targets<'a>(
        &'a mut self,
        clicked_idx: usize,
        albums: &'a [AlbumUIViewData],
    ) -> impl Iterator<Item = AlbumsRow<'a>> + 'a {
        let targets = self.common.get_batch_target_indices(clicked_idx);
        self.expansion.rows_at(targets, albums, |a| &a.id)
    }

    /// Resolve a row-menu click on flattened row `clicked_idx`. Batch entries
    /// act on the selection when the clicked row is part of it; the rest act
    /// on the clicked row alone.
    fn context_menu_action(
        &mut self,
        clicked_idx: usize,
        entry: LibraryContextEntry,
        albums: &[AlbumUIViewData],
    ) -> AlbumsAction {
        use LibraryContextEntry as Entry;
        let row = self.expansion.get_entry_at(clicked_idx, albums, |a| &a.id);
        match (entry, row) {
            (Entry::ShufflePlay, _) => AlbumsAction::PlayBatch(
                self.menu_targets(clicked_idx, albums)
                    .map(Self::batch_item)
                    .collect(),
                true,
            ),
            (Entry::AddToQueue, _) => AlbumsAction::AddBatchToQueue(
                self.menu_targets(clicked_idx, albums)
                    .map(Self::batch_item)
                    .collect(),
            ),
            (Entry::AddToPlaylist, _) => AlbumsAction::AddBatchToPlaylist(
                self.menu_targets(clicked_idx, albums)
                    .map(Self::batch_item)
                    .collect(),
            ),
            (Entry::AddToMix, _) => AlbumsAction::AddBatchToMix(
                self.menu_targets(clicked_idx, albums)
                    .map(Self::trawl_seed)
                    .collect(),
            ),
            (Entry::GetInfo, Some(SlotListEntry::Parent(album))) => {
                AlbumsAction::ShowInfo(Box::new(self.album_info_item(album)))
            }
            (Entry::GetInfo, Some(SlotListEntry::Child(song, _))) => {
                AlbumsAction::ShowInfo(Box::new(InfoModalItem::from_song_view_data(song)))
            }
            (Entry::ShowInFolder, Some(SlotListEntry::Parent(album))) => {
                AlbumsAction::ShowInFolder(album.id.clone())
            }
            (Entry::ShowInFolder, Some(SlotListEntry::Child(song, _))) => {
                AlbumsAction::ShowSongInFolder(song.path.clone())
            }
            (Entry::FindSimilar, Some(SlotListEntry::Parent(album))) => {
                AlbumsAction::FindSimilar(album.id.clone(), album.name.clone())
            }
            (Entry::FindSimilar, Some(SlotListEntry::Child(song, _))) => {
                AlbumsAction::FindSimilar(song.id.clone(), song.title.clone())
            }
            // Like the Songs view: a track without an artist has no top songs.
            (Entry::TopSongs, Some(SlotListEntry::Child(song, _))) => {
                if song.artist.is_empty() {
                    AlbumsAction::None
                } else {
                    AlbumsAction::TopSongs(song.artist.clone())
                }
            }
            // Album rows don't offer Top Songs; the rest are other views' entries.
            (Entry::TopSongs, Some(SlotListEntry::Parent(_)))
            | (Entry::GetInfo | Entry::ShowInFolder | Entry::FindSimilar | Entry::TopSongs, None)
            | (
                Entry::Separator
                | Entry::RemoveFromPlaylist
                | Entry::ReplaceQueueWithAllFound
                | Entry::AddAllFoundToQueue
                | Entry::AddAllFoundToPlaylist,
                _,
            ) => AlbumsAction::None,
        }
    }

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
            slot_list: AlbumsMessage::SlotList,
            on_center: |center| center.map_or(AlbumsAction::None, |idx| {
                AlbumsAction::LoadLargeArtwork(idx.to_string())
            }),
            activate: |row, row_idx, force| match row {
                SlotListEntry::Parent(_) => AlbumsAction::PlayAlbum(row_idx.to_string(), force),
                SlotListEntry::Child(_, album_id) => AlbumsAction::PlayAlbumFromTrack(
                    album_id,
                    self.expansion.count_children_before(row_idx, albums, |a| &a.id),
                    force,
                ),
            },
            batch_item: Self::batch_item,
            play_selection: AlbumsAction::PlaySelection,
            add_to_queue: AlbumsAction::AddBatchToQueue,
            refresh: AlbumsAction::RefreshViewData,
            center_on_playing: AlbumsAction::CenterOnPlaying,
            none: AlbumsAction::None,
        ) {
            Ok(handled) => handled,
            Err(message) => match message {
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
                // Data loading messages (handled at root level, no action needed here)
                AlbumsMessage::ArtworkLoaded(_, _) => (Task::none(), AlbumsAction::None),
                AlbumsMessage::LargeArtworkLoaded(_, _) => (Task::none(), AlbumsAction::None),
                // Routed up to root in `handle_albums` before this match runs;
                // arm exists only for exhaustiveness.
                AlbumsMessage::SetOpenMenu(_) => (Task::none(), AlbumsAction::None),
                AlbumsMessage::Roulette => (Task::none(), AlbumsAction::None),
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
                AlbumsMessage::ContextMenuAction(clicked_idx, entry) => (
                    Task::none(),
                    self.context_menu_action(clicked_idx, entry, albums),
                ),
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
                    let new_value = self.column_visibility.toggle(col);
                    (
                        Task::none(),
                        AlbumsAction::ColumnVisibilityChanged(col, new_value),
                    )
                }
                // Routed up to root in `handle_albums` before this match runs.
                AlbumsMessage::ArtworkColumnDrag(_)
                | AlbumsMessage::ArtworkColumnVerticalDrag(_) => (Task::none(), AlbumsAction::None),
                // Handled by `impl_expansion_update!` above.
                AlbumsMessage::SlotList(_)
                | AlbumsMessage::ExpandCenter
                | AlbumsMessage::CollapseExpansion
                | AlbumsMessage::TracksLoaded(..)
                | AlbumsMessage::SortModeSelected(_)
                | AlbumsMessage::ToggleSortOrder
                | AlbumsMessage::SearchQueryChanged(_)
                | AlbumsMessage::SearchFocused(_) => (Task::none(), AlbumsAction::None),
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
        let (_, action) = page.update(
            AlbumsMessage::SlotList(crate::widgets::SlotListPageMessage::CenterOnPlaying),
            0,
            &empty_albums,
        );

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
