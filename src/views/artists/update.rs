//! Artists view — `impl ArtistsPage { fn update }`.
//!
//! Handler for `ArtistsMessage`. View rendering lives in `view.rs`;
//! types live in `mod.rs`.

use iced::Task;
use nokkvi_data::{backend::artists::ArtistUIViewData, types::ItemKind};

use super::{super::expansion::SlotListEntry, ArtistsAction, ArtistsMessage, ArtistsPage};
use crate::widgets::SlotListPageMessage;

impl ArtistsPage {
    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: ArtistsMessage,
        total_items: usize,
        artists: &[ArtistUIViewData],
    ) -> (Task<ArtistsMessage>, ArtistsAction) {
        // Shift+Enter on a centered child album row: route to the
        // cross-view "navigate to Albums + expand there" path instead of
        // doing an inline 3rd-tier expansion. Parent rows keep the
        // toggle-collapse behaviour the macro provides; child rows would
        // otherwise just collapse the outer expansion (the 2-tier
        // `handle_expand_center` semantics), which is the wrong choice
        // here — we want drill-down, not collapse.
        if matches!(message, ArtistsMessage::ExpandCenter) && self.expansion.is_expanded() {
            let total = self.expansion.flattened_len(artists);
            let center = self
                .common
                .get_center_item_index(total)
                .and_then(|idx| self.expansion.get_entry_at(idx, artists, |a| &a.id));
            if let Some(SlotListEntry::Child(album, _)) = center {
                return (
                    Task::none(),
                    ArtistsAction::NavigateAndExpandAlbum(album.id.clone()),
                );
            }
        }

        match super::super::impl_expansion_update!(
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
            Ok(result) => result,
            Err(msg) => match msg {
                ArtistsMessage::FocusAndExpand(offset) => {
                    let len = self.expansion.flattened_len(artists);
                    self.common
                        .handle_slot_click(offset, len, Default::default());
                    if let Some(parent_id) =
                        self.expansion
                            .handle_expand_center(artists, |a| &a.id, &mut self.common)
                    {
                        (Task::none(), ArtistsAction::ExpandArtist(parent_id))
                    } else {
                        (Task::none(), ArtistsAction::None)
                    }
                }
                ArtistsMessage::NavigateAndExpandAlbum(album_id) => (
                    Task::none(),
                    ArtistsAction::NavigateAndExpandAlbum(album_id),
                ),
                ArtistsMessage::SlotList(msg) => match msg {
                    SlotListPageMessage::NavigateUp => {
                        self.expansion.handle_navigate_up(artists, &mut self.common);
                        (Task::none(), ArtistsAction::LoadLargeArtwork)
                    }
                    SlotListPageMessage::NavigateDown => {
                        self.expansion
                            .handle_navigate_down(artists, &mut self.common);
                        (Task::none(), ArtistsAction::LoadLargeArtwork)
                    }
                    SlotListPageMessage::SetOffset(offset, modifiers) => {
                        self.expansion.handle_select_offset(
                            offset,
                            modifiers,
                            artists,
                            &mut self.common,
                        );
                        (Task::none(), ArtistsAction::LoadLargeArtwork)
                    }
                    SlotListPageMessage::ScrollSeek(offset) => {
                        self.expansion
                            .handle_set_offset(offset, artists, &mut self.common);
                        // Mid-drag: update viewport offset only. Artwork +
                        // page-fetch deferred to the SeekSettled debounce, which
                        // synthesises a SetOffset message that emits LoadLargeArtwork.
                        (Task::none(), ArtistsAction::None)
                    }
                    SlotListPageMessage::ClickPlay(offset) => {
                        let len = self.expansion.flattened_len(artists);
                        self.common.handle_set_offset(offset, len);
                        self.update(
                            ArtistsMessage::SlotList(SlotListPageMessage::ActivateCenter),
                            total_items,
                            artists,
                        )
                    }
                    SlotListPageMessage::SelectionToggle(offset) => {
                        let flattened = self.expansion.flattened_len(artists);
                        self.common.handle_selection_toggle(offset, flattened);
                        (Task::none(), ArtistsAction::None)
                    }
                    SlotListPageMessage::SelectAllToggle => {
                        let flattened = self.expansion.flattened_len(artists);
                        self.common.handle_select_all_toggle(flattened);
                        (Task::none(), ArtistsAction::None)
                    }
                    SlotListPageMessage::ActivateCenter => {
                        let total = self.expansion.flattened_len(artists);
                        if let Some(center_idx) = self.common.get_center_item_index(total) {
                            self.common.slot_list.flash_center();
                            match self.expansion.get_entry_at(center_idx, artists, |a| &a.id) {
                                Some(SlotListEntry::Child(album, _)) => {
                                    (Task::none(), ArtistsAction::PlayAlbum(album.id.clone()))
                                }
                                Some(SlotListEntry::Parent(_)) => (
                                    Task::none(),
                                    ArtistsAction::PlayArtist(center_idx.to_string()),
                                ),
                                None => (Task::none(), ArtistsAction::None),
                            }
                        } else {
                            (Task::none(), ArtistsAction::None)
                        }
                    }
                    SlotListPageMessage::AddCenterToQueue => {
                        use nokkvi_data::types::batch::BatchItem;
                        let total = self.expansion.flattened_len(artists);

                        let target_indices = self.common.get_queue_target_indices(total);

                        if target_indices.is_empty() {
                            return (Task::none(), ArtistsAction::None);
                        }

                        let payload =
                            super::super::expansion::build_batch_payload(target_indices, |i| {
                                match self.expansion.get_entry_at(i, artists, |a| &a.id) {
                                    Some(SlotListEntry::Parent(artist)) => {
                                        Some(BatchItem::Artist(artist.id.clone()))
                                    }
                                    Some(SlotListEntry::Child(album, _)) => {
                                        Some(BatchItem::Album(album.id.clone()))
                                    }
                                    None => None,
                                }
                            });

                        (Task::none(), ArtistsAction::AddBatchToQueue(payload))
                    }
                    SlotListPageMessage::RefreshViewData => {
                        (Task::none(), ArtistsAction::RefreshViewData)
                    }
                    SlotListPageMessage::CenterOnPlaying => {
                        (Task::none(), ArtistsAction::CenterOnPlaying)
                    }
                    // Sort/search are handled by impl_expansion_update! above;
                    // these arms exist only for exhaustiveness.
                    SlotListPageMessage::SearchQueryChanged(_)
                    | SlotListPageMessage::SearchFocused(_)
                    | SlotListPageMessage::SortModeSelected(_)
                    | SlotListPageMessage::ToggleSortOrder => (Task::none(), ArtistsAction::None),
                },

                // Routed up to root in `handle_artists` before this match runs;
                // arm exists only for exhaustiveness.
                ArtistsMessage::SetOpenMenu(_) => (Task::none(), ArtistsAction::None),
                ArtistsMessage::Roulette => (Task::none(), ArtistsAction::None),
                ArtistsMessage::NavigateAndFilter(view, filter) => {
                    (Task::none(), ArtistsAction::NavigateAndFilter(view, filter))
                }
                ArtistsMessage::ToggleColumnVisible(col) => {
                    let new_value = !self.column_visibility.get(col);
                    self.column_visibility.set(col, new_value);
                    (
                        Task::none(),
                        ArtistsAction::ColumnVisibilityChanged(col, new_value),
                    )
                }
                ArtistsMessage::ClickSetRating(item_index, rating) => {
                    use nokkvi_data::utils::formatters::compute_rating_toggle;
                    match self.expansion.get_entry_at(item_index, artists, |a| &a.id) {
                        Some(SlotListEntry::Child(album, _)) => {
                            let current = album.rating.unwrap_or(0) as usize;
                            let new_rating = compute_rating_toggle(current, rating);
                            (
                                Task::none(),
                                ArtistsAction::SetRating(
                                    album.id.clone(),
                                    ItemKind::Album,
                                    new_rating,
                                ),
                            )
                        }
                        Some(SlotListEntry::Parent(artist)) => {
                            let current = artist.rating.unwrap_or(0) as usize;
                            let new_rating = compute_rating_toggle(current, rating);
                            (
                                Task::none(),
                                ArtistsAction::SetRating(
                                    artist.id.clone(),
                                    ItemKind::Artist,
                                    new_rating,
                                ),
                            )
                        }
                        None => (Task::none(), ArtistsAction::None),
                    }
                }
                ArtistsMessage::ClickToggleStar(item_index) => {
                    match self.expansion.get_entry_at(item_index, artists, |a| &a.id) {
                        Some(SlotListEntry::Child(album, _)) => (
                            Task::none(),
                            ArtistsAction::ToggleStar(
                                album.id.clone(),
                                ItemKind::Album,
                                !album.is_starred,
                            ),
                        ),
                        Some(SlotListEntry::Parent(artist)) => (
                            Task::none(),
                            ArtistsAction::ToggleStar(
                                artist.id.clone(),
                                ItemKind::Artist,
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
                                super::super::expansion::build_batch_payload(target_indices, |i| {
                                    match self.expansion.get_entry_at(i, artists, |a| &a.id) {
                                        Some(SlotListEntry::Parent(artist)) => {
                                            Some(BatchItem::Artist(artist.id.clone()))
                                        }
                                        Some(SlotListEntry::Child(album, _)) => {
                                            Some(BatchItem::Album(album.id.clone()))
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
                        _ => match self.expansion.get_entry_at(clicked_idx, artists, |a| &a.id) {
                            Some(SlotListEntry::Parent(artist)) => match entry {
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
                                LibraryContextEntry::FindSimilar => (
                                    Task::none(),
                                    ArtistsAction::FindSimilar(
                                        artist.id.clone(),
                                        format!("Similar to: {}", artist.name),
                                    ),
                                ),
                                LibraryContextEntry::TopSongs => (
                                    Task::none(),
                                    ArtistsAction::TopSongs(
                                        artist.name.clone(),
                                        format!("Top Songs: {}", artist.name),
                                    ),
                                ),
                                _ => (Task::none(), ArtistsAction::None),
                            },
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
                                    (Task::none(), ArtistsAction::ShowInfo(Box::new(item)))
                                }
                                LibraryContextEntry::ShowInFolder => (
                                    Task::none(),
                                    ArtistsAction::ShowAlbumInFolder(album.id.clone()),
                                ),
                                LibraryContextEntry::Separator => {
                                    (Task::none(), ArtistsAction::None)
                                }
                                LibraryContextEntry::FindSimilar => {
                                    let aid = album.artist.clone();
                                    (
                                        Task::none(),
                                        ArtistsAction::FindSimilar(
                                            aid,
                                            format!("Similar to: {}", album.name),
                                        ),
                                    )
                                }
                                _ => (Task::none(), ArtistsAction::None),
                            },
                            None => (Task::none(), ArtistsAction::None),
                        },
                    }
                }
                // Common arms already handled by macro above
                _ => (Task::none(), ArtistsAction::None),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::artists::ArtistsColumn;

    #[test]
    fn toggle_column_visible_flips_state_and_emits_action() {
        let mut page = ArtistsPage::default();
        let artists: Vec<ArtistUIViewData> = Vec::new();

        let (_t, action) = page.update(
            ArtistsMessage::ToggleColumnVisible(ArtistsColumn::Plays),
            0,
            &artists,
        );
        assert!(!page.column_visibility.plays);
        assert!(matches!(
            action,
            ArtistsAction::ColumnVisibilityChanged(ArtistsColumn::Plays, false)
        ));
    }
}
