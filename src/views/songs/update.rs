//! Songs view — `impl SongsPage { fn update, fn sort_mode_to_api_string }`.
//!
//! Handler for `SongsMessage`. Songs is the only non-expansion slot-list
//! view, so this update fn does not use `impl_expansion_update!`.
//! View rendering lives in `view.rs`; types live in `mod.rs`.

use iced::Task;
use nokkvi_data::backend::songs::SongUIViewData;

use super::{SongsAction, SongsMessage, SongsPage};
use crate::widgets::{SlotListPageAction, view_header::SortMode};

impl SongsPage {
    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: SongsMessage,
        songs: &[SongUIViewData],
    ) -> (Task<SongsMessage>, SongsAction) {
        let total_items = songs.len();

        match message {
            SongsMessage::SlotList(msg) => {
                use crate::widgets::SlotListPageMessage;

                // For navigate up/down and set-offset, we need to trigger artwork loading
                // after the state mutation. Detect these cases before delegating.
                let needs_artwork_load = matches!(
                    msg,
                    SlotListPageMessage::NavigateUp
                        | SlotListPageMessage::NavigateDown
                        | SlotListPageMessage::SetOffset(_, _)
                );

                match self.common.handle(msg, total_items) {
                    SlotListPageAction::ActivateCenter => {
                        if !self.common.slot_list.selected_indices.is_empty() {
                            use nokkvi_data::types::batch::{BatchItem, BatchPayload};
                            let payload = self
                                .common
                                .slot_list
                                .selected_indices
                                .iter()
                                .filter_map(|&index| {
                                    songs.get(index).map(|s| {
                                        let item: nokkvi_data::types::song::Song = s.clone().into();
                                        BatchItem::Song(Box::new(item))
                                    })
                                })
                                .fold(BatchPayload::new(), |p, i| p.with_item(i));
                            (Task::none(), SongsAction::PlayBatch(payload))
                        } else if let Some(center_idx) =
                            self.common.get_center_item_index(total_items)
                        {
                            self.common.slot_list.flash_center();
                            (Task::none(), SongsAction::PlaySongFromIndex(center_idx))
                        } else {
                            (Task::none(), SongsAction::None)
                        }
                    }
                    SlotListPageAction::AddCenterToQueue => {
                        use nokkvi_data::types::batch::BatchItem;

                        let target_indices = self.common.get_queue_target_indices(total_items);

                        if target_indices.is_empty() {
                            return (Task::none(), SongsAction::None);
                        }

                        let payload =
                            super::super::expansion::build_batch_payload(target_indices, |i| {
                                songs.get(i).map(|s| {
                                    let item: nokkvi_data::types::song::Song = s.clone().into();
                                    BatchItem::Song(Box::new(item))
                                })
                            });

                        (Task::none(), SongsAction::AddBatchToQueue(payload))
                    }
                    SlotListPageAction::SearchChanged(q) => {
                        (Task::none(), SongsAction::SearchChanged(q))
                    }
                    SlotListPageAction::SortModeChanged(m) => {
                        (Task::none(), SongsAction::SortModeChanged(m))
                    }
                    SlotListPageAction::SortOrderChanged(b) => {
                        (Task::none(), SongsAction::SortOrderChanged(b))
                    }
                    SlotListPageAction::RefreshViewData => {
                        (Task::none(), SongsAction::RefreshViewData)
                    }
                    SlotListPageAction::CenterOnPlaying => {
                        (Task::none(), SongsAction::CenterOnPlaying)
                    }
                    SlotListPageAction::None => {
                        // For navigation/set-offset actions, trigger artwork load for the
                        // new center item if one is now in focus.
                        if needs_artwork_load
                            && let Some(center_idx) = self.common.get_center_item_index(total_items)
                            && let Some(song) = songs.get(center_idx)
                            && let Some(album_id) = &song.album_id
                        {
                            return (
                                Task::none(),
                                SongsAction::LoadLargeArtwork(album_id.clone()),
                            );
                        }
                        (Task::none(), SongsAction::None)
                    }
                }
            }

            SongsMessage::ClickSetRating(item_index, rating) => {
                if let Some(song) = songs.get(item_index) {
                    use nokkvi_data::utils::formatters::compute_rating_toggle;
                    let current = song.rating.unwrap_or(0) as usize;
                    let new_rating = compute_rating_toggle(current, rating);
                    (
                        Task::none(),
                        SongsAction::SetRating(song.id.clone(), new_rating),
                    )
                } else {
                    (Task::none(), SongsAction::None)
                }
            }
            SongsMessage::ClickToggleStar(item_index) => {
                if let Some(song) = songs.get(item_index) {
                    return (
                        Task::none(),
                        SongsAction::ToggleStar(song.id.clone(), !song.is_starred),
                    );
                }
                (Task::none(), SongsAction::None)
            }
            SongsMessage::ContextMenuAction(clicked_idx, entry) => {
                use nokkvi_data::types::batch::BatchItem;

                use crate::widgets::context_menu::LibraryContextEntry;

                let target_indices = self.common.get_batch_target_indices(clicked_idx);

                let payload = super::super::expansion::build_batch_payload(target_indices, |i| {
                    songs.get(i).map(|s| {
                        let item: nokkvi_data::types::song::Song = s.clone().into();
                        BatchItem::Song(Box::new(item))
                    })
                });

                if let Some(song) = songs.get(clicked_idx) {
                    match entry {
                        LibraryContextEntry::AddToQueue => {
                            (Task::none(), SongsAction::AddBatchToQueue(payload))
                        }
                        LibraryContextEntry::AddToPlaylist => {
                            // AddToPlaylist backend takes a Vec<String> of song IDs, or a batch?
                            // We will emit AddBatchToPlaylist but for now, if Batch doesn't fit AddToPlaylist perfectly,
                            // we can map payload -> IDs. Let's just pass payload.
                            (Task::none(), SongsAction::AddBatchToPlaylist(payload))
                        }
                        LibraryContextEntry::GetInfo => {
                            use nokkvi_data::types::info_modal::InfoModalItem;
                            let item = InfoModalItem::from_song_view_data(song);
                            (Task::none(), SongsAction::ShowInfo(Box::new(item)))
                        }
                        LibraryContextEntry::ShowInFolder => {
                            (Task::none(), SongsAction::ShowInFolder(song.path.clone()))
                        }
                        LibraryContextEntry::FindSimilar => (
                            Task::none(),
                            SongsAction::FindSimilar(song.id.clone(), song.title.clone()),
                        ),
                        LibraryContextEntry::TopSongs => {
                            let artist = &song.artist;
                            if !artist.is_empty() {
                                (
                                    Task::none(),
                                    SongsAction::TopSongs(
                                        artist.clone(),
                                        format!("Top Songs: {artist}"),
                                    ),
                                )
                            } else {
                                (Task::none(), SongsAction::None)
                            }
                        }
                        LibraryContextEntry::Separator
                        | LibraryContextEntry::ReplaceQueueWithAllFound
                        | LibraryContextEntry::AddAllFoundToQueue
                        | LibraryContextEntry::AddAllFoundToPlaylist => {
                            (Task::none(), SongsAction::None)
                        }
                    }
                } else {
                    (Task::none(), SongsAction::None)
                }
            }

            // Routed up to root in `handle_songs` before this match runs;
            // arm exists only for exhaustiveness.
            SongsMessage::SetOpenMenu(_) => (Task::none(), SongsAction::None),
            SongsMessage::Roulette => (Task::none(), SongsAction::None),
            SongsMessage::ArtworkColumnDrag(_) | SongsMessage::ArtworkColumnVerticalDrag(_) => {
                // Intercepted at root before reaching this update; never reached.
                (Task::none(), SongsAction::None)
            }
            SongsMessage::RefreshArtwork(album_id) => {
                (Task::none(), SongsAction::RefreshArtwork(album_id))
            }
            SongsMessage::NavigateAndFilter(view, filter) => {
                (Task::none(), SongsAction::NavigateAndFilter(view, filter))
            }
            SongsMessage::NavigateAndExpandAlbum(album_id) => {
                (Task::none(), SongsAction::NavigateAndExpandAlbum(album_id))
            }
            SongsMessage::NavigateAndExpandArtist(artist_id) => (
                Task::none(),
                SongsAction::NavigateAndExpandArtist(artist_id),
            ),
            SongsMessage::NavigateAndExpandGenre(genre_id) => {
                (Task::none(), SongsAction::NavigateAndExpandGenre(genre_id))
            }
            SongsMessage::ToggleColumnVisible(col) => {
                let new_value = !self.column_visibility.get(col);
                self.column_visibility.set(col, new_value);
                (
                    Task::none(),
                    SongsAction::ColumnVisibilityChanged(col, new_value),
                )
            }
        }
    }

    /// Convert SortMode to API string for ViewModel.
    /// Thin shim — the unified mapping lives in `views/sort_api.rs`.
    pub fn sort_mode_to_api_string(sort_mode: SortMode) -> &'static str {
        super::super::sort_api::sort_mode_to_api_string(crate::View::Songs, sort_mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::songs::SongsColumn;

    #[test]
    fn songs_toggle_column_visible_flips_state_and_emits_action() {
        let mut page = SongsPage::default();
        let empty: Vec<SongUIViewData> = vec![];
        let (_t, action) = page.update(
            SongsMessage::ToggleColumnVisible(SongsColumn::Plays),
            &empty,
        );
        assert!(page.column_visibility.plays);
        assert!(matches!(
            action,
            SongsAction::ColumnVisibilityChanged(SongsColumn::Plays, true)
        ));

        // Genre default is off → toggle ON, message carries Genre+true.
        let (_t, action) = page.update(
            SongsMessage::ToggleColumnVisible(SongsColumn::Genre),
            &empty,
        );
        assert!(page.column_visibility.genre);
        assert!(matches!(
            action,
            SongsAction::ColumnVisibilityChanged(SongsColumn::Genre, true)
        ));
    }

    #[test]
    fn songs_slot_list_sort_mode_selected_updates_state_and_emits_action() {
        use crate::widgets::{SlotListPageMessage, view_header::SortMode};
        let mut page = SongsPage::default();
        let empty: Vec<SongUIViewData> = vec![];
        let (_t, action) = page.update(
            SongsMessage::SlotList(SlotListPageMessage::SortModeSelected(SortMode::MostPlayed)),
            &empty,
        );
        assert_eq!(page.common.current_sort_mode, SortMode::MostPlayed);
        assert!(matches!(
            action,
            SongsAction::SortModeChanged(SortMode::MostPlayed)
        ));
    }
}
