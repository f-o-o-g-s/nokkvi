//! Queue view — `impl QueuePage { fn update }`.
//!
//! Handler for `QueueMessage`. Queue does not use `impl_expansion_update!`
//! and does not implement `HasCommonAction` — the queue's action enum
//! tracks queue-specific concerns (drag reorder, playlist edit/save, etc.)
//! that are entirely separate from the library views.
//!
//! View rendering lives in `view.rs`; types live in `mod.rs`.

use iced::Task;
use nokkvi_data::backend::queue::QueueSongUIViewData;
use tracing::{debug, trace};

use super::{QueueAction, QueueContextEntry, QueueMessage, QueuePage};
use crate::widgets::{SlotListPageAction, drag_column::DragEvent};

impl QueuePage {
    /// Update internal state and return actions for root
    pub fn update(
        &mut self,
        message: QueueMessage,
        queue_songs: &[QueueSongUIViewData],
    ) -> (Task<QueueMessage>, QueueAction) {
        let total_items = queue_songs.len();
        match message {
            QueueMessage::SlotList(msg) => {
                match self.common.handle(msg, total_items) {
                    SlotListPageAction::ActivateCenter => {
                        // Play the centered song
                        if let Some(center_idx) = self.common.get_center_item_index(total_items) {
                            self.common.slot_list.flash_center();
                            (Task::none(), QueueAction::PlaySong(center_idx))
                        } else {
                            (Task::none(), QueueAction::None)
                        }
                    }
                    SlotListPageAction::SearchChanged(q) => {
                        (Task::none(), QueueAction::SearchChanged(q))
                    }
                    SlotListPageAction::SortOrderChanged(b) => {
                        (Task::none(), QueueAction::SortOrderChanged(b))
                    }
                    SlotListPageAction::None => (Task::none(), QueueAction::None),
                    // Queue has no AddCenterToQueue / RefreshViewData / CenterOnPlaying / SortModeChanged
                    // via SlotList arm (SortModeSelected stays per-view with QueueSortMode).
                    _ => (Task::none(), QueueAction::None),
                }
            }
            QueueMessage::FocusCurrentPlaying(queue_index, flash) => {
                // Auto-scroll slot list to center the currently playing track by queue index
                // Bubble up to handler which has access to queue_songs to find the slot
                trace!(
                    " [QUEUE PAGE] FocusCurrentPlaying({}) called, current_offset={}",
                    queue_index, self.common.slot_list.viewport_offset
                );
                (Task::none(), QueueAction::FocusOnSong(queue_index, flash))
            }
            QueueMessage::NavigateAndFilter(view, filter) => {
                (Task::none(), QueueAction::NavigateAndFilter(view, filter))
            }
            QueueMessage::NavigateAndExpandAlbum(album_id) => {
                (Task::none(), QueueAction::NavigateAndExpandAlbum(album_id))
            }
            QueueMessage::NavigateAndExpandArtist(artist_id) => (
                Task::none(),
                QueueAction::NavigateAndExpandArtist(artist_id),
            ),
            QueueMessage::NavigateAndExpandGenre(genre_id) => {
                (Task::none(), QueueAction::NavigateAndExpandGenre(genre_id))
            }
            QueueMessage::SortModeSelected(sort_mode) => {
                self.queue_sort_mode = sort_mode;
                (Task::none(), QueueAction::SortModeChanged(sort_mode))
            }
            QueueMessage::ToggleColumnVisible(col) => {
                let new_value = !self.column_visibility.get(col);
                self.column_visibility.set(col, new_value);
                (
                    Task::none(),
                    QueueAction::ColumnVisibilityChanged(col, new_value),
                )
            }

            // Routed up to root in `handle_queue` before this match runs;
            // arm exists only for exhaustiveness.
            QueueMessage::SetOpenMenu(_) => (Task::none(), QueueAction::None),
            QueueMessage::Roulette => (Task::none(), QueueAction::None),
            QueueMessage::ArtworkColumnDrag(_) | QueueMessage::ArtworkColumnVerticalDrag(_) => {
                // Intercepted at root before reaching this update; never reached.
                (Task::none(), QueueAction::None)
            }
            QueueMessage::OpenDefaultPlaylistPicker => {
                (Task::none(), QueueAction::OpenDefaultPlaylistPicker)
            }
            QueueMessage::ClickSetRating(item_index, rating) => {
                if let Some(song) = queue_songs.get(item_index) {
                    let current = song.rating.unwrap_or(0) as usize;
                    let new_rating = if rating == current {
                        rating.saturating_sub(1)
                    } else {
                        rating
                    };
                    (
                        Task::none(),
                        QueueAction::SetRating(song.id.clone(), new_rating),
                    )
                } else {
                    (Task::none(), QueueAction::None)
                }
            }
            QueueMessage::ClickToggleStar(item_index) => {
                if let Some(song) = queue_songs.get(item_index) {
                    (
                        Task::none(),
                        QueueAction::ToggleStar(song.id.clone(), !song.starred),
                    )
                } else {
                    (Task::none(), QueueAction::None)
                }
            }
            QueueMessage::DragReorder(drag_event) => {
                // Drag is allowed in any sort mode, but blocked during active search
                let drag_allowed = self.common.search_query.is_empty();

                match drag_event {
                    DragEvent::Picked { .. } if !drag_allowed => (
                        Task::none(),
                        QueueAction::ShowToast("Clear search to reorder queue".to_string()),
                    ),
                    DragEvent::Dropped {
                        index,
                        target_index,
                    } if drag_allowed => {
                        // Translate slot indices to absolute item indices using the
                        // same effective_center logic that build_slot_list_slots uses for
                        // rendering. Simple `viewport_offset + slot` is wrong because
                        // it doesn't account for the center_slot offset.
                        let from = self.common.slot_list.slot_to_item_index(index, total_items);
                        let to = self
                            .common
                            .slot_list
                            .slot_to_item_index_for_drop(target_index, total_items);
                        debug!(
                            "\u{1f4e6} [QUEUE] Drag reorder: slot {}\u{2192}{} \u{2192} item {:?}\u{2192}{:?} \\
                             (viewport_offset={}, slot_count={}, total={})",
                            index,
                            target_index,
                            from,
                            to,
                            self.common.slot_list.viewport_offset,
                            self.common.slot_list.slot_count,
                            total_items,
                        );

                        // Multi-selection batch drag: if selected_indices has multiple
                        // items and the dragged item is one of them, move the whole batch.
                        let selected = &self.common.slot_list.selected_indices;
                        if selected.len() > 1
                            && from.is_some_and(|f| selected.contains(&f))
                            && let Some(t) = to
                        {
                            let indices: Vec<usize> = selected.iter().copied().collect();
                            self.common.clear_multi_selection();
                            (Task::none(), QueueAction::MoveBatch { indices, target: t })
                        } else {
                            match (from, to) {
                                (Some(f), Some(t)) => {
                                    // Keep highlight on the moved item at its new position
                                    let insert_at = if f < t { t - 1 } else { t };
                                    self.common.slot_list.set_selected(insert_at, total_items);
                                    (Task::none(), QueueAction::MoveItem { from: f, to: t })
                                }
                                _ => {
                                    debug!(
                                        "\u{1f4e6} [QUEUE] Drag dropped on empty slot, ignoring"
                                    );
                                    (Task::none(), QueueAction::None)
                                }
                            }
                        }
                    }
                    DragEvent::Picked { index } if drag_allowed => {
                        // Check if the picked item is part of an active multi-selection.
                        // If yes, preserve the selection (batch drag). If not, highlight
                        // only the dragged item (single drag).
                        if let Some(item_index) =
                            self.common.slot_list.slot_to_item_index(index, total_items)
                            && !self.common.slot_list.selected_indices.contains(&item_index)
                        {
                            self.common.slot_list.set_selected(item_index, total_items);
                        }
                        (Task::none(), QueueAction::None)
                    }
                    _ => (Task::none(), QueueAction::None),
                }
            }
            QueueMessage::ContextMenuAction(clicked_idx, entry) => match entry {
                QueueContextEntry::Play => {
                    self.common.handle_set_offset(clicked_idx, total_items);
                    (Task::none(), QueueAction::PlaySong(clicked_idx))
                }
                QueueContextEntry::RemoveFromQueue | QueueContextEntry::PlayNext => {
                    let target_indices = self.common.evaluate_context_menu(clicked_idx);
                    self.common.clear_multi_selection();

                    // Resolve filtered indices → per-row `entry_id`s at the
                    // boundary so downstream code is both index-free *and*
                    // duplicate-aware. Two rows of the same song_id carry
                    // distinct entry_ids, so a right-click targets a single
                    // row without taking sibling duplicates with it.
                    let target_entry_ids: Vec<u64> = target_indices
                        .iter()
                        .filter_map(|&idx| queue_songs.get(idx).map(|s| s.entry_id))
                        .collect();

                    match entry {
                        QueueContextEntry::RemoveFromQueue => {
                            (Task::none(), QueueAction::RemoveFromQueue(target_entry_ids))
                        }
                        QueueContextEntry::PlayNext => {
                            (Task::none(), QueueAction::PlayNext(target_entry_ids))
                        }
                        _ => unreachable!(),
                    }
                }
                QueueContextEntry::AddToPlaylist => {
                    let target_indices = self.common.evaluate_context_menu(clicked_idx);
                    self.common.clear_multi_selection();
                    let target_songs: Vec<String> = target_indices
                        .iter()
                        .filter_map(|&idx| queue_songs.get(idx).map(|s| s.id.clone()))
                        .collect();
                    if target_songs.is_empty() {
                        (Task::none(), QueueAction::None)
                    } else {
                        (Task::none(), QueueAction::AddToPlaylist(target_songs))
                    }
                }
                QueueContextEntry::Separator => (Task::none(), QueueAction::None),
                QueueContextEntry::SaveAsPlaylist => (Task::none(), QueueAction::SaveAsPlaylist),
                QueueContextEntry::OpenBrowsingPanel => {
                    (Task::none(), QueueAction::OpenBrowsingPanel)
                }
                QueueContextEntry::GetInfo => (Task::none(), QueueAction::ShowInfo(clicked_idx)),
                QueueContextEntry::ShowInFolder => {
                    (Task::none(), QueueAction::ShowInFolder(clicked_idx))
                }
                QueueContextEntry::FindSimilar => {
                    (Task::none(), QueueAction::FindSimilar(clicked_idx))
                }
                QueueContextEntry::TopSongs => (Task::none(), QueueAction::TopSongs(clicked_idx)),
            },
            QueueMessage::SavePlaylist => (Task::none(), QueueAction::SavePlaylist),
            QueueMessage::DiscardEdits => (Task::none(), QueueAction::DiscardEdits),
            QueueMessage::PlaylistNameChanged(name) => {
                (Task::none(), QueueAction::PlaylistNameChanged(name))
            }
            QueueMessage::PlaylistCommentChanged(comment) => {
                (Task::none(), QueueAction::PlaylistCommentChanged(comment))
            }
            QueueMessage::PlaylistEditPublicToggled(value) => {
                (Task::none(), QueueAction::PlaylistEditPublicToggled(value))
            }
            QueueMessage::EditPlaylist => (Task::none(), QueueAction::EditPlaylist),
            QueueMessage::QuickSavePlaylist => (Task::none(), QueueAction::SaveAsPlaylist),
            QueueMessage::RefreshArtwork(album_id) => {
                (Task::none(), QueueAction::RefreshArtwork(album_id))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::queue::QueueColumn;

    #[test]
    fn toggle_column_visible_flips_state() {
        let mut page = QueuePage::default();
        let songs: Vec<QueueSongUIViewData> = Vec::new();

        // Stars: true → false → true.
        let (_t, action) = page.update(
            QueueMessage::ToggleColumnVisible(QueueColumn::Stars),
            &songs,
        );
        assert!(!page.column_visibility.stars);
        assert!(matches!(
            action,
            QueueAction::ColumnVisibilityChanged(QueueColumn::Stars, false)
        ));

        let (_t, action) = page.update(
            QueueMessage::ToggleColumnVisible(QueueColumn::Stars),
            &songs,
        );
        assert!(page.column_visibility.stars);
        assert!(matches!(
            action,
            QueueAction::ColumnVisibilityChanged(QueueColumn::Stars, true)
        ));

        // Album and Duration use the same path, just spot-check Album.
        let (_t, _action) = page.update(
            QueueMessage::ToggleColumnVisible(QueueColumn::Album),
            &songs,
        );
        assert!(!page.column_visibility.album);

        // Genre default is off → toggle ON, message carries Genre+true.
        let (_t, action) = page.update(
            QueueMessage::ToggleColumnVisible(QueueColumn::Genre),
            &songs,
        );
        assert!(page.column_visibility.genre);
        assert!(matches!(
            action,
            QueueAction::ColumnVisibilityChanged(QueueColumn::Genre, true)
        ));
        // Other columns unaffected.
        assert!(page.column_visibility.stars);
        assert!(page.column_visibility.duration);
        assert!(page.column_visibility.love);

        // Love toggles independently and emits its own action.
        let (_t, action) =
            page.update(QueueMessage::ToggleColumnVisible(QueueColumn::Love), &songs);
        assert!(!page.column_visibility.love);
        assert!(matches!(
            action,
            QueueAction::ColumnVisibilityChanged(QueueColumn::Love, false)
        ));
    }
}
