//! Artists view — `impl ArtistsPage { fn update }`.
//!
//! Handler for `ArtistsMessage`. View rendering lives in `view.rs`;
//! types live in `mod.rs`.

use std::collections::HashSet;

use iced::Task;
use nokkvi_data::{
    backend::{albums::AlbumUIViewData, artists::ArtistUIViewData},
    types::{
        ItemKind,
        batch::{BatchItem, BatchPayload},
        info_modal::InfoModalItem,
        trawl::TrawlSeed,
    },
};

use super::{
    super::expansion::SlotListEntry, ArtistsAction, ArtistsMessage, ArtistsPage, DiscographyRow,
    discography,
};
use crate::widgets::context_menu::LibraryContextEntry;

type ArtistsRow<'a> = SlotListEntry<&'a ArtistUIViewData, &'a DiscographyRow>;

/// What a batch action takes from the rows it was given: an artist, or an
/// album. A section header stands for its albums, so it never appears here.
enum BatchTarget<'a> {
    Artist(&'a ArtistUIViewData),
    Album(&'a AlbumUIViewData),
}

impl ArtistsPage {
    /// What `rows` stand for in a play / queue / playlist / mix batch, in
    /// display order. An album counts once however often it is reached.
    fn batch_targets<'a>(rows: impl Iterator<Item = ArtistsRow<'a>>) -> Vec<BatchTarget<'a>> {
        let mut seen_albums: HashSet<&str> = HashSet::new();
        let mut targets = Vec::new();
        for row in rows {
            match row {
                SlotListEntry::Parent(artist) => targets.push(BatchTarget::Artist(artist)),
                SlotListEntry::Child(DiscographyRow::Album(album), _) => {
                    if seen_albums.insert(&album.id) {
                        targets.push(BatchTarget::Album(album));
                    }
                }
                SlotListEntry::Child(DiscographyRow::Section(_), _) => {}
            }
        }
        targets
    }

    /// The batch `rows` stand for (see [`Self::batch_targets`]).
    fn batch_payload<'a>(rows: impl Iterator<Item = ArtistsRow<'a>>) -> BatchPayload {
        Self::batch_targets(rows)
            .into_iter()
            .map(|target| match target {
                BatchTarget::Artist(artist) => BatchItem::Artist(artist.id.clone()),
                BatchTarget::Album(album) => BatchItem::Album(album.id.clone()),
            })
            .collect()
    }

    /// The Trawl seeds `rows` add to the crate (see [`Self::batch_targets`]).
    fn trawl_seeds<'a>(rows: impl Iterator<Item = ArtistsRow<'a>>) -> Vec<TrawlSeed> {
        Self::batch_targets(rows)
            .into_iter()
            .map(|target| match target {
                BatchTarget::Artist(artist) => {
                    TrawlSeed::from_artist(artist.id.clone(), artist.name.clone())
                }
                BatchTarget::Album(album) => TrawlSeed::from_album(
                    album.id.clone(),
                    album.name.clone(),
                    album.artist.clone(),
                ),
            })
            .collect()
    }

    /// The batch a row-menu entry acts on: the selection when the clicked
    /// row is part of it, else the clicked row alone (clears the selection).
    fn menu_payload(&mut self, clicked_idx: usize, artists: &[ArtistUIViewData]) -> BatchPayload {
        let targets = self.common.get_batch_target_indices(clicked_idx);
        Self::batch_payload(self.expansion.rows_at(targets, artists, |a| &a.id))
    }

    /// [`Self::menu_payload`] as Trawl seeds.
    fn menu_seeds(&mut self, clicked_idx: usize, artists: &[ArtistUIViewData]) -> Vec<TrawlSeed> {
        let targets = self.common.get_batch_target_indices(clicked_idx);
        Self::trawl_seeds(self.expansion.rows_at(targets, artists, |a| &a.id))
    }

    /// Resolve a row-menu click on flattened row `clicked_idx`. Batch entries
    /// act on the selection when the clicked row is part of it; the rest act
    /// on the clicked row alone.
    fn context_menu_action(
        &mut self,
        clicked_idx: usize,
        entry: LibraryContextEntry,
        artists: &[ArtistUIViewData],
    ) -> ArtistsAction {
        use LibraryContextEntry as Entry;
        let row = self.expansion.get_entry_at(clicked_idx, artists, |a| &a.id);
        match (entry, row) {
            (Entry::ShufflePlay, _) => {
                ArtistsAction::PlayBatch(self.menu_payload(clicked_idx, artists), true)
            }
            (Entry::AddToQueue, _) => {
                ArtistsAction::AddBatchToQueue(self.menu_payload(clicked_idx, artists))
            }
            (Entry::AddToPlaylist, _) => {
                ArtistsAction::AddBatchToPlaylist(self.menu_payload(clicked_idx, artists))
            }
            (Entry::AddToMix, _) => {
                ArtistsAction::AddBatchToMix(self.menu_seeds(clicked_idx, artists))
            }
            (Entry::GetInfo, Some(SlotListEntry::Parent(artist))) => {
                ArtistsAction::ShowInfo(Box::new(InfoModalItem::from_artist_view_data(artist)))
            }
            (Entry::GetInfo, Some(SlotListEntry::Child(DiscographyRow::Album(album), _))) => {
                ArtistsAction::ShowInfo(Box::new(InfoModalItem::from_album_view_data(album, None)))
            }
            (Entry::ShowInFolder, Some(SlotListEntry::Child(DiscographyRow::Album(album), _))) => {
                ArtistsAction::ShowAlbumInFolder(album.id.clone())
            }
            (Entry::FindSimilar, Some(SlotListEntry::Parent(artist))) => {
                ArtistsAction::FindSimilar(artist.id.clone(), artist.name.clone())
            }
            (Entry::FindSimilar, Some(SlotListEntry::Child(DiscographyRow::Album(album), _))) => {
                ArtistsAction::FindSimilar(album.id.clone(), album.name.clone())
            }
            (Entry::TopSongs, Some(SlotListEntry::Parent(artist))) => {
                ArtistsAction::TopSongs(artist.name.clone(), Some(artist.id.clone()))
            }
            (Entry::SimilarArtists, Some(SlotListEntry::Parent(artist))) => {
                ArtistsAction::SimilarArtists(crate::views::ArtistRef::new(
                    &artist.id,
                    &artist.name,
                ))
            }
            (
                Entry::SimilarArtists,
                Some(SlotListEntry::Child(DiscographyRow::Album(album), _)),
            ) => ArtistsAction::SimilarArtists(crate::views::ArtistRef::new(
                &album.artist_id,
                &album.artist,
            )),
            // An artist has no folder of its own, album rows don't offer Top
            // Songs, and a section header offers only the batch entries; the
            // rest are other views' entries.
            (Entry::ShowInFolder, Some(SlotListEntry::Parent(_)))
            | (Entry::TopSongs, Some(SlotListEntry::Child(..)))
            | (
                Entry::GetInfo | Entry::ShowInFolder | Entry::FindSimilar | Entry::SimilarArtists,
                Some(SlotListEntry::Child(DiscographyRow::Section(_), _)),
            )
            | (
                Entry::GetInfo
                | Entry::ShowInFolder
                | Entry::FindSimilar
                | Entry::TopSongs
                | Entry::SimilarArtists,
                None,
            )
            | (
                Entry::Separator
                | Entry::RemoveFromPlaylist
                | Entry::ReplaceQueueWithAllFound
                | Entry::AddAllFoundToQueue
                | Entry::AddAllFoundToPlaylist,
                _,
            ) => ArtistsAction::None,
        }
    }

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
            if let Some(SlotListEntry::Child(DiscographyRow::Album(album), _)) = center {
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
            children_loaded: ArtistsMessage::AlbumsLoaded => |id, albums| discography::build_rows(
                albums,
                &id,
                self.group_by_release_type,
                &self.collapsed_sections,
            ),
            sort_selected: ArtistsMessage::SortModeSelected => ArtistsAction::SortModeChanged,
            toggle_sort: ArtistsMessage::ToggleSortOrder => ArtistsAction::SortOrderChanged,
            search_changed: ArtistsMessage::SearchQueryChanged => ArtistsAction::SearchChanged,
            search_focused: ArtistsMessage::SearchFocused,
            slot_list: ArtistsMessage::SlotList,
            on_center: |_center| ArtistsAction::LoadLargeArtwork,
            activate: |row, row_idx, force| match row {
                // The root resolves the artist by its index in the parent buffer.
                SlotListEntry::Parent(_) => ArtistsAction::PlayArtist(
                    self.expansion.parent_index_at(row_idx, artists, |a| &a.id).to_string(),
                    force,
                ),
                SlotListEntry::Child(DiscographyRow::Album(album), _) => {
                    ArtistsAction::PlayAlbum(album.id.clone(), force)
                }
                SlotListEntry::Child(DiscographyRow::Section(_), _) => ArtistsAction::None,
            },
            batch: |rows| Self::batch_payload(rows),
            play_selection: ArtistsAction::PlaySelection,
            add_to_queue: ArtistsAction::AddBatchToQueue,
            refresh: ArtistsAction::RefreshViewData,
            center_on_playing: ArtistsAction::CenterOnPlaying,
            none: ArtistsAction::None,
        ) {
            Ok(handled) => handled,
            Err(message) => match message {
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
                ArtistsMessage::NavigateAndExpandArtist(artist_id) => (
                    Task::none(),
                    ArtistsAction::NavigateAndExpandArtist(artist_id),
                ),

                ArtistsMessage::NavigateAndFilter(view, filter) => {
                    (Task::none(), ArtistsAction::NavigateAndFilter(view, filter))
                }
                ArtistsMessage::ToggleColumnVisible(col) => {
                    let new_value = self.column_visibility.toggle(col);
                    (
                        Task::none(),
                        ArtistsAction::ColumnVisibilityChanged(col, new_value),
                    )
                }
                ArtistsMessage::ClickSetRating(item_index, rating) => {
                    use nokkvi_data::utils::formatters::compute_rating_toggle;
                    match self.expansion.get_entry_at(item_index, artists, |a| &a.id) {
                        Some(SlotListEntry::Child(DiscographyRow::Album(album), _)) => {
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
                        // A section header carries no rating.
                        Some(SlotListEntry::Child(DiscographyRow::Section(_), _)) | None => {
                            (Task::none(), ArtistsAction::None)
                        }
                    }
                }
                ArtistsMessage::ClickToggleStar(item_index) => {
                    match self.expansion.get_entry_at(item_index, artists, |a| &a.id) {
                        Some(SlotListEntry::Child(DiscographyRow::Album(album), _)) => (
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
                        // A section header carries no heart.
                        Some(SlotListEntry::Child(DiscographyRow::Section(_), _)) | None => {
                            (Task::none(), ArtistsAction::None)
                        }
                    }
                }
                ArtistsMessage::ContextMenuAction(clicked_idx, entry) => (
                    Task::none(),
                    self.context_menu_action(clicked_idx, entry, artists),
                ),
                // Routed up to root in `handle_artists` before this match runs.
                ArtistsMessage::SetOpenMenu(_)
                | ArtistsMessage::Roulette
                | ArtistsMessage::OpenExternalUrl(_)
                | ArtistsMessage::ArtworkColumnDrag(_)
                | ArtistsMessage::ArtworkColumnVerticalDrag(_)
                // Handled by `impl_expansion_update!` above.
                | ArtistsMessage::SlotList(_)
                | ArtistsMessage::ExpandCenter
                | ArtistsMessage::CollapseExpansion
                | ArtistsMessage::AlbumsLoaded(..)
                | ArtistsMessage::SortModeSelected(_)
                | ArtistsMessage::ToggleSortOrder
                | ArtistsMessage::SearchQueryChanged(_)
                | ArtistsMessage::SearchFocused(_) => (Task::none(), ArtistsAction::None),
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
