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
                    SlotListPageAction::ActivateCenter(_) => {
                        // Play the centered song (the queue is already the play order,
                        // so a one-shot Shuffle Play directive does not apply here)
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
            QueueMessage::FocusCurrentPlaying(entry_id, flash) => {
                // Auto-scroll slot list to center the currently playing row by
                // its per-row entry_id (drift-immune across optimistic mutations).
                trace!(
                    " [QUEUE PAGE] FocusCurrentPlaying(entry_id={}) called, current_offset={}",
                    entry_id, self.common.slot_list.viewport_offset
                );
                (Task::none(), QueueAction::FocusOnSong(entry_id, flash))
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
                let new_value = self.column_visibility.toggle(col);
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
            QueueMessage::OpenTrawl => (Task::none(), QueueAction::OpenTrawl),
            QueueMessage::ClickSetRating(item_index, rating) => {
                if let Some(song) = queue_songs.get(item_index) {
                    use nokkvi_data::utils::formatters::compute_rating_toggle;
                    let current = song.rating.unwrap_or(0) as usize;
                    let new_rating = compute_rating_toggle(current, rating);
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
                    DragEvent::Picked { .. } if !drag_allowed => {
                        // A search activated since the drag began; drop any
                        // half-captured source (and live ghost/scroll state) so a
                        // later (search-cleared) drop can't consume stale state.
                        self.clear_drag();
                        (
                            Task::none(),
                            QueueAction::ShowToast("Clear search to reorder queue".to_string()),
                        )
                    }
                    DragEvent::Dropped {
                        index,
                        target_index,
                    } if drag_allowed => {
                        // SOURCE: snapshotted by per-row entry_id at PICK time, so
                        // it survives a mid-drag viewport shift (playback
                        // auto-follow re-centering) or a queue reload. The frozen
                        // pick SLOT (`index`) is intentionally unused.
                        let _ = index;
                        let source = self.drag_source.take();
                        // Gesture ended — clear the live ghost/scroll state
                        // (`source` already taken above).
                        self.clear_drag();

                        // DESTINATION: follows the live cursor against the CURRENT
                        // viewport. `slot_to_item` reads the stored slot_count,
                        // which `handle_queue` resyncs (collapse-aware) immediately
                        // before this update — so it matches the live render even
                        // with the auto-hide toolbar collapsed, and computes the
                        // boundary `effective_center` correctly. A past-end /
                        // empty-area drop maps to `None`; treat it as "append at
                        // end", mirroring the cross-pane `HoveredSlot::Empty` drop.
                        let to = self
                            .common
                            .slot_list
                            .slot_to_item_index_for_drop(target_index, total_items)
                            .unwrap_or(total_items);

                        debug!(
                            "\u{1f4e6} [QUEUE] Drag drop: target slot {} \u{2192} item {} \
                             (source rows {:?}, viewport_offset={}, slot_count={}, total={})",
                            target_index,
                            to,
                            source.as_deref().map(<[u64]>::len),
                            self.common.slot_list.viewport_offset,
                            self.common.slot_list.slot_count,
                            total_items,
                        );

                        match source {
                            // Multi-selection batch drag.
                            Some(ids) if ids.len() > 1 => {
                                self.common.clear_multi_selection();
                                (
                                    Task::none(),
                                    QueueAction::MoveBatch {
                                        entry_ids: ids,
                                        target: to,
                                    },
                                )
                            }
                            // Single-row drag.
                            Some(ids) => match ids.first() {
                                Some(&source_entry_id) => (
                                    Task::none(),
                                    QueueAction::MoveItem {
                                        source_entry_id,
                                        to,
                                    },
                                ),
                                None => (Task::none(), QueueAction::None),
                            },
                            None => {
                                debug!(
                                    "\u{1f4e6} [QUEUE] Drag drop with no captured source, ignoring"
                                );
                                (Task::none(), QueueAction::None)
                            }
                        }
                    }
                    DragEvent::Picked { index } if drag_allowed => {
                        // Snapshot the dragged source row(s) by per-row entry_id
                        // NOW. `slot_to_item_index` reads the stored slot_count,
                        // which `handle_queue` resyncs (collapse-aware) immediately
                        // before this update, so it matches the live render. The
                        // entry_id snapshot then survives any later viewport shift.
                        if let Some(item_index) =
                            self.common.slot_list.slot_to_item_index(index, total_items)
                        {
                            // A multi-selection drag moves the whole batch only
                            // when the picked row is one of the selected rows.
                            let is_batch = self.common.slot_list.selected_indices.len() > 1
                                && self.common.slot_list.selected_indices.contains(&item_index);
                            if is_batch {
                                let ids: Vec<u64> = self
                                    .common
                                    .slot_list
                                    .selected_indices
                                    .iter()
                                    .filter_map(|&i| queue_songs.get(i).map(|s| s.entry_id))
                                    .collect();
                                self.drag_source = (!ids.is_empty()).then_some(ids);
                            } else {
                                // Single drag: highlight only this row and
                                // capture its identity.
                                self.common.slot_list.set_selected(item_index, total_items);
                                self.drag_source =
                                    queue_songs.get(item_index).map(|s| vec![s.entry_id]);
                            }
                        } else {
                            // Picked an empty/padding slot — nothing to drag.
                            self.drag_source = None;
                        }
                        (Task::none(), QueueAction::None)
                    }
                    // Cursor moved during an active drag: track the live cursor,
                    // edge band, and drop-target slot for the floating ghost +
                    // tick auto-scroll, but ONLY once a pick was accepted
                    // (`drag_source` set). A search-swallowed pick leaves it
                    // None, so this stays inert; the consumers (ghost render, tick
                    // auto-scroll) additionally gate on search being empty. This
                    // explicit arm MUST precede the `_` catch-all, which would
                    // otherwise clear the captured source on every cursor move.
                    DragEvent::Dragged {
                        cursor,
                        edge,
                        target_slot,
                    } => {
                        if !drag_allowed {
                            // A search activated mid-drag — cancel the gesture so
                            // it can't resume when the search clears, matching the
                            // editor (whose top-level guard clears on every event
                            // during a search).
                            self.clear_drag();
                        } else if self.drag_source.is_some() {
                            self.drag_cursor = Some(cursor);
                            self.drag_edge = edge;
                            self.drag_target_slot = Some(target_slot);
                        }
                        (Task::none(), QueueAction::None)
                    }
                    // Dropped while a search is active: swallow it and clear all
                    // stale drag state so a later drop can't replay it.
                    _ => {
                        self.clear_drag();
                        (Task::none(), QueueAction::None)
                    }
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
                QueueContextEntry::AddToMix => {
                    let target_indices = self.common.evaluate_context_menu(clicked_idx);
                    self.common.clear_multi_selection();
                    // Queue rows are concrete songs — rebuild a playable Song
                    // from the row projection (streaming keys on the id).
                    let seeds: Vec<nokkvi_data::types::trawl::TrawlSeed> = target_indices
                        .iter()
                        .filter_map(|&idx| queue_songs.get(idx))
                        .map(|row| {
                            let song = nokkvi_data::types::song::Song {
                                id: row.id.clone(),
                                title: row.title.clone(),
                                artist: row.artist.clone(),
                                artist_id: Some(row.artist_id.clone()),
                                album: row.album.clone(),
                                album_id: Some(row.album_id.clone()),
                                duration: row.duration_seconds,
                                genre: (!row.genre.is_empty()).then(|| row.genre.clone()),
                                starred: row.starred,
                                rating: row.rating,
                                play_count: row.play_count,
                                updated_at: row.updated_at.clone(),
                                ..Default::default()
                            };
                            nokkvi_data::types::trawl::TrawlSeed::new(
                                nokkvi_data::types::batch::BatchItem::Song(Box::new(song)),
                                row.title.clone(),
                                row.artist.clone(),
                            )
                        })
                        .collect();
                    if seeds.is_empty() {
                        (Task::none(), QueueAction::None)
                    } else {
                        (Task::none(), QueueAction::AddToMix(seeds))
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
            QueueMessage::EditPlaylist => (Task::none(), QueueAction::EditPlaylist),
            QueueMessage::QuickSavePlaylist => (Task::none(), QueueAction::SaveAsPlaylist),
            QueueMessage::RefreshArtwork(album_id) => {
                (Task::none(), QueueAction::RefreshArtwork(album_id))
            }
            QueueMessage::PlaylistStripHoverEnter => {
                self.playlist_strip_expanded = true;
                (Task::none(), QueueAction::None)
            }
            QueueMessage::PlaylistStripHoverExit => {
                self.playlist_strip_expanded = false;
                (Task::none(), QueueAction::None)
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

    /// Build a queue row with a known rating so we can exercise the
    /// rating-toggle handler without dragging in the full UI fixture.
    fn make_song(id: &str, rating: Option<u32>) -> QueueSongUIViewData {
        QueueSongUIViewData {
            id: id.to_string(),
            entry_id: 0,
            track_number: 1,
            title: "t".into(),
            artist: "a".into(),
            artist_id: "ai".into(),
            album: "al".into(),
            album_id: "alid".into(),
            artwork_url: String::new(),
            updated_at: None,
            duration: "0:00".into(),
            duration_seconds: 0,
            genre: String::new(),
            starred: false,
            rating,
            play_count: None,
            searchable_lower: "t".into(),
        }
    }

    /// Queue's `ClickSetRating` was the lone outlier that inlined the
    /// `if rating == current { rating - 1 } else { rating }` table — now
    /// routed through `compute_rating_toggle` to match Albums / Artists /
    /// Songs. This test pins parity by computing each row's expected new
    /// rating with `compute_rating_toggle` and asserting the handler
    /// produces the same `QueueAction::SetRating(id, new)`.
    #[test]
    fn queue_click_set_rating_matches_compute_rating_toggle() {
        use nokkvi_data::utils::formatters::compute_rating_toggle;

        let songs = vec![
            make_song("s0", Some(3)), // current 3
            make_song("s1", Some(1)), // current 1 — same-click goes to 0
            make_song("s2", None),    // current 0 — new rating
            make_song("s3", Some(5)), // current 5 — same-click decrements
        ];
        let mut page = QueuePage::default();

        // (item_index, clicked) tuples that probe each compute_rating_toggle branch.
        let cases = [(0, 5), (0, 3), (1, 1), (2, 4), (3, 5)];
        for (idx, clicked) in cases {
            let current = songs[idx].rating.unwrap_or(0) as usize;
            let expected = compute_rating_toggle(current, clicked);

            let (_t, action) = page.update(QueueMessage::ClickSetRating(idx, clicked), &songs);
            match action {
                QueueAction::SetRating(id, new) => {
                    assert_eq!(id, songs[idx].id);
                    assert_eq!(
                        new, expected,
                        "compute_rating_toggle parity broke for idx={idx} clicked={clicked}"
                    );
                }
                other => panic!("expected QueueAction::SetRating, got {other:?}"),
            }
        }
    }

    #[test]
    fn queue_click_set_rating_out_of_bounds_returns_none_action() {
        let songs = vec![make_song("s0", Some(2))];
        let mut page = QueuePage::default();
        let (_t, action) = page.update(QueueMessage::ClickSetRating(42, 4), &songs);
        assert!(matches!(action, QueueAction::None));
    }

    #[test]
    fn playlist_strip_hover_toggles_expanded() {
        let mut page = QueuePage::default();
        let songs: Vec<QueueSongUIViewData> = Vec::new();
        assert!(!page.playlist_strip_expanded, "default is collapsed");

        let (_t, action) = page.update(QueueMessage::PlaylistStripHoverEnter, &songs);
        assert!(page.playlist_strip_expanded, "hover-enter expands");
        assert!(matches!(action, QueueAction::None), "no root action");

        let (_t, action) = page.update(QueueMessage::PlaylistStripHoverExit, &songs);
        assert!(!page.playlist_strip_expanded, "hover-exit collapses");
        assert!(matches!(action, QueueAction::None), "no root action");
    }
}
