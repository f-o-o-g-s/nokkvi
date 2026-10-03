//! Genres view — `impl GenresPage { fn update, fn resolve_artwork_action }`.
//!
//! Handler for `GenresMessage` plus the artwork-resolution helper that
//! drives `LoadArtwork` after navigation. View rendering lives in `view.rs`;
//! types live in `mod.rs`.

use iced::Task;
use nokkvi_data::{
    backend::{albums::AlbumUIViewData, genres::GenreUIViewData},
    types::{ItemKind, batch::BatchItem, info_modal::InfoModalItem, trawl::TrawlSeed},
};

use super::{super::expansion::SlotListEntry, GenresAction, GenresMessage, GenresPage};
use crate::widgets::{SlotListPageMessage, context_menu::LibraryContextEntry};

type GenresRow<'a> = SlotListEntry<&'a GenreUIViewData, &'a AlbumUIViewData>;

impl GenresPage {
    /// What a row stands for in a play / queue / playlist batch (genres are
    /// name-keyed, like the batch pipeline).
    fn batch_item(row: GenresRow<'_>) -> BatchItem {
        match row {
            SlotListEntry::Parent(genre) => BatchItem::Genre(genre.name.clone()),
            SlotListEntry::Child(album, _) => BatchItem::Album(album.id.clone()),
        }
    }

    /// The Trawl seed a row adds to the crate.
    fn trawl_seed(row: GenresRow<'_>) -> TrawlSeed {
        match row {
            SlotListEntry::Parent(genre) => {
                TrawlSeed::from_genre(genre.name.clone(), genre.album_count)
            }
            SlotListEntry::Child(album, _) => {
                TrawlSeed::from_album(album.id.clone(), album.name.clone(), album.artist.clone())
            }
        }
    }

    /// The rows a batch menu entry acts on: the selection when the clicked
    /// row is part of it, else the clicked row alone (clears the selection).
    fn menu_targets<'a>(
        &'a mut self,
        clicked_idx: usize,
        genres: &'a [GenreUIViewData],
    ) -> impl Iterator<Item = GenresRow<'a>> + 'a {
        let targets = self.common.get_batch_target_indices(clicked_idx);
        self.expansion.rows_at(targets, genres, |g| &g.id)
    }

    /// Resolve a row-menu click on flattened row `clicked_idx`. Batch entries
    /// act on the selection when the clicked row is part of it; the rest act
    /// on the clicked row alone.
    fn context_menu_action(
        &mut self,
        clicked_idx: usize,
        entry: LibraryContextEntry,
        genres: &[GenreUIViewData],
    ) -> GenresAction {
        use LibraryContextEntry as Entry;
        let row = self.expansion.get_entry_at(clicked_idx, genres, |g| &g.id);
        match (entry, row) {
            (Entry::ShufflePlay, _) => GenresAction::PlayBatch(
                self.menu_targets(clicked_idx, genres)
                    .map(Self::batch_item)
                    .collect(),
                true,
            ),
            (Entry::AddToQueue, _) => GenresAction::AddBatchToQueue(
                self.menu_targets(clicked_idx, genres)
                    .map(Self::batch_item)
                    .collect(),
            ),
            (Entry::AddToPlaylist, _) => GenresAction::AddBatchToPlaylist(
                self.menu_targets(clicked_idx, genres)
                    .map(Self::batch_item)
                    .collect(),
            ),
            (Entry::AddToMix, _) => GenresAction::AddBatchToMix(
                self.menu_targets(clicked_idx, genres)
                    .map(Self::trawl_seed)
                    .collect(),
            ),
            (Entry::GetInfo, Some(SlotListEntry::Child(album, _))) => {
                GenresAction::ShowInfo(Box::new(InfoModalItem::from_album_view_data(album, None)))
            }
            (Entry::ShowInFolder, Some(SlotListEntry::Child(album, _))) => {
                GenresAction::ShowAlbumInFolder(album.id.clone())
            }
            (Entry::FindSimilar, Some(SlotListEntry::Child(album, _))) => {
                GenresAction::FindSimilar(album.id.clone(), album.name.clone())
            }
            // A genre has no info modal, folder or similar-songs seed, and
            // album rows don't offer Top Songs; the rest are other views' entries.
            (
                Entry::GetInfo | Entry::ShowInFolder | Entry::FindSimilar | Entry::TopSongs,
                Some(SlotListEntry::Parent(_)) | None,
            )
            | (Entry::TopSongs, Some(SlotListEntry::Child(..)))
            | (
                Entry::Separator
                | Entry::RemoveFromPlaylist
                | Entry::ReplaceQueueWithAllFound
                | Entry::AddAllFoundToQueue
                | Entry::AddAllFoundToPlaylist,
                _,
            ) => GenresAction::None,
        }
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

        match super::super::impl_expansion_update!(
            self, message, genres, total_items,
            id_fn: |g| &g.id,
            expand_center: GenresMessage::ExpandCenter => GenresAction::ExpandGenre,
            collapse: GenresMessage::CollapseExpansion,
            children_loaded: GenresMessage::AlbumsLoaded,
            sort_selected: GenresMessage::SortModeSelected => GenresAction::SortModeChanged,
            toggle_sort: GenresMessage::ToggleSortOrder => GenresAction::SortOrderChanged,
            search_changed: GenresMessage::SearchQueryChanged => GenresAction::SearchChanged,
            search_focused: GenresMessage::SearchFocused,
            slot_list_wrap: GenresMessage::SlotList,
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
                        (Task::none(), GenresAction::ExpandGenre(parent_id))
                    } else {
                        (Task::none(), GenresAction::None)
                    }
                }
                GenresMessage::NavigateAndExpandAlbum(album_id) => {
                    (Task::none(), GenresAction::NavigateAndExpandAlbum(album_id))
                }
                GenresMessage::SlotList(msg) => match msg {
                    SlotListPageMessage::NavigateUp => {
                        self.expansion.handle_navigate_up(genres, &mut self.common);
                        let action = self.resolve_artwork_action(genres);
                        (Task::none(), action)
                    }
                    SlotListPageMessage::NavigateDown => {
                        self.expansion
                            .handle_navigate_down(genres, &mut self.common);
                        let action = self.resolve_artwork_action(genres);
                        (Task::none(), action)
                    }
                    SlotListPageMessage::SetOffset(offset, modifiers) => {
                        self.expansion.handle_select_offset(
                            offset,
                            modifiers,
                            genres,
                            &mut self.common,
                        );
                        let action = self.resolve_artwork_action(genres);
                        (Task::none(), action)
                    }
                    SlotListPageMessage::ScrollSeek(offset) => {
                        self.expansion
                            .handle_set_offset(offset, genres, &mut self.common);
                        (Task::none(), GenresAction::None)
                    }
                    SlotListPageMessage::ClickPlay(offset) => {
                        self.expansion
                            .handle_set_offset(offset, genres, &mut self.common);
                        self.update(
                            GenresMessage::SlotList(SlotListPageMessage::ActivateCenter(false)),
                            total_items,
                            genres,
                        )
                    }
                    SlotListPageMessage::SelectionToggle(offset) => {
                        let flattened = self.expansion.flattened_len(genres);
                        self.common.handle_selection_toggle(offset, flattened);
                        (Task::none(), GenresAction::None)
                    }
                    SlotListPageMessage::SelectAllToggle => {
                        let flattened = self.expansion.flattened_len(genres);
                        self.common.handle_select_all_toggle(flattened);
                        (Task::none(), GenresAction::None)
                    }
                    SlotListPageMessage::ActivateCenter(force) => {
                        let total = self.expansion.flattened_len(genres);
                        if let Some(center_idx) = self.common.get_center_item_index(total) {
                            self.common.slot_list.flash_center();
                            match self.expansion.get_entry_at(center_idx, genres, |g| &g.id) {
                                Some(SlotListEntry::Child(album, _)) => (
                                    Task::none(),
                                    GenresAction::PlayAlbum(album.id.clone(), force),
                                ),
                                Some(SlotListEntry::Parent(genre)) => (
                                    Task::none(),
                                    GenresAction::PlayGenre(genre.name.clone(), force),
                                ),
                                None => (Task::none(), GenresAction::None),
                            }
                        } else {
                            (Task::none(), GenresAction::None)
                        }
                    }
                    SlotListPageMessage::AddCenterToQueue => {
                        use nokkvi_data::types::batch::BatchItem;
                        let total = self.expansion.flattened_len(genres);

                        let target_indices = self.common.get_queue_target_indices(total);

                        if target_indices.is_empty() {
                            return (Task::none(), GenresAction::None);
                        }

                        let payload =
                            super::super::expansion::build_batch_payload(target_indices, |i| {
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
                    SlotListPageMessage::RefreshViewData => {
                        (Task::none(), GenresAction::RefreshViewData)
                    }
                    SlotListPageMessage::CenterOnPlaying => {
                        (Task::none(), GenresAction::CenterOnPlaying)
                    }
                    // Exhaustiveness: variants handled by macro above come through
                    // the Ok arm; these are forwarded by view-level emit sites that
                    // wrap common messages — treat as no-op here.
                    #[allow(unreachable_patterns)]
                    _ => (Task::none(), GenresAction::None),
                },
                GenresMessage::ClickToggleStar(item_index) => {
                    match self.expansion.get_entry_at(item_index, genres, |g| &g.id) {
                        Some(SlotListEntry::Child(album, _)) => (
                            Task::none(),
                            GenresAction::ToggleStar(
                                album.id.clone(),
                                ItemKind::Album,
                                !album.is_starred,
                            ),
                        ),
                        Some(SlotListEntry::Parent(_genre)) => {
                            // Genres don't have starred state
                            (Task::none(), GenresAction::None)
                        }
                        None => (Task::none(), GenresAction::None),
                    }
                }
                // Routed up to root in `handle_genres` before this match runs;
                // arm exists only for exhaustiveness.
                GenresMessage::SetOpenMenu(_) => (Task::none(), GenresAction::None),
                GenresMessage::Roulette => (Task::none(), GenresAction::None),
                GenresMessage::NavigateAndFilter(view, filter) => {
                    (Task::none(), GenresAction::NavigateAndFilter(view, filter))
                }
                GenresMessage::NavigateAndExpandArtist(artist_id) => (
                    Task::none(),
                    GenresAction::NavigateAndExpandArtist(artist_id),
                ),
                GenresMessage::ToggleColumnVisible(col) => {
                    let new_value = self.column_visibility.toggle(col);
                    (
                        Task::none(),
                        GenresAction::ColumnVisibilityChanged(col, new_value),
                    )
                }
                GenresMessage::ContextMenuAction(clicked_idx, entry) => (
                    Task::none(),
                    self.context_menu_action(clicked_idx, entry, genres),
                ),
                // Common arms already handled by macro above
                _ => (Task::none(), GenresAction::None),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::genres::GenresColumn;

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
