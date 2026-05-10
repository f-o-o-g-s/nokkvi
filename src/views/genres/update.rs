//! Genres view — `impl GenresPage { fn update, fn resolve_artwork_action }`.
//!
//! Handler for `GenresMessage` plus the artwork-resolution helper that
//! drives `LoadArtwork` after navigation. View rendering lives in `view.rs`;
//! types live in `mod.rs`.

use iced::Task;
use nokkvi_data::{backend::genres::GenreUIViewData, types::ItemKind};

use super::{super::expansion::SlotListEntry, GenresAction, GenresMessage, GenresPage};
use crate::widgets::SlotListPageMessage;

impl GenresPage {
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
                            GenresMessage::SlotList(SlotListPageMessage::ActivateCenter),
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
                    SlotListPageMessage::ActivateCenter => {
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
                    SlotListPageMessage::AddToQueue => {
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
                    SlotListPageMessage::Refresh => (Task::none(), GenresAction::RefreshViewData),
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
