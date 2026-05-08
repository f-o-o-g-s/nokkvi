//! Hotkey action handlers
//!
//! Split into domain-specific submodules:
//! - `star_rating`: Star/favorite and rating handlers
//! - `queue`: Queue management (add, remove, clear, shuffle, save, move)
//! - `navigation`: Search, sort, and center-on-playing

mod navigation;
mod queue;
mod star_rating;

use iced::Task;
use nokkvi_data::types::info_modal::InfoModalItem;
use tracing::{debug, trace};

use crate::{
    Nokkvi, View,
    app_message::{HotkeyMessage, Message},
    views,
    views::expansion::SlotListEntry,
};

impl Nokkvi {
    /// Get the current view as a `&dyn ViewPage` for trait-based dispatch.
    /// Returns None for Settings (which doesn't implement ViewPage).
    ///
    /// In playlist edit mode with browser focus, returns the browsing panel's
    /// active view page so all existing hotkey handlers work on the browser pane.
    pub(crate) fn current_view_page(&self) -> Option<&dyn views::ViewPage> {
        // Pane-aware routing: when editing with browser focus, delegate to the active tab
        if self.browsing_panel.is_some()
            && self.pane_focus == crate::state::PaneFocus::Browser
            && let Some(panel) = &self.browsing_panel
        {
            return match panel.active_view {
                views::BrowsingView::Albums => Some(&self.albums_page),
                views::BrowsingView::Songs => Some(&self.songs_page),
                views::BrowsingView::Artists => Some(&self.artists_page),
                views::BrowsingView::Genres => Some(&self.genres_page),
                views::BrowsingView::Similar => Some(&self.similar_page),
            };
        }

        self.view_page(self.current_view)
    }

    /// Get the current view as a `&mut dyn ViewPage` for trait-based dispatch.
    /// Returns None for Settings (which doesn't implement ViewPage).
    ///
    /// In playlist edit mode with browser focus, returns the browsing panel's
    /// active view page so all existing hotkey handlers work on the browser pane.
    pub(crate) fn current_view_page_mut(&mut self) -> Option<&mut dyn views::ViewPage> {
        // Pane-aware routing: when editing with browser focus, delegate to the active tab
        if self.browsing_panel.is_some()
            && self.pane_focus == crate::state::PaneFocus::Browser
            && let Some(panel) = &self.browsing_panel
        {
            return match panel.active_view {
                views::BrowsingView::Albums => Some(&mut self.albums_page),
                views::BrowsingView::Songs => Some(&mut self.songs_page),
                views::BrowsingView::Artists => Some(&mut self.artists_page),
                views::BrowsingView::Genres => Some(&mut self.genres_page),
                views::BrowsingView::Similar => Some(&mut self.similar_page),
            };
        }

        self.view_page_mut(self.current_view)
    }

    /// Look up a page by explicit `View` — no pane-focus routing.
    /// Used by scrollbar timer handlers that always target a specific view.
    pub(crate) fn view_page(&self, view: View) -> Option<&dyn views::ViewPage> {
        match view {
            View::Albums => Some(&self.albums_page),
            View::Artists => Some(&self.artists_page),
            View::Songs => Some(&self.songs_page),
            View::Genres => Some(&self.genres_page),
            View::Playlists => Some(&self.playlists_page),
            View::Queue => Some(&self.queue_page),
            View::Radios => Some(&self.radios_page),
            View::Settings => None,
        }
    }

    /// Look up a page by explicit `View` (mutable) — no pane-focus routing.
    pub(crate) fn view_page_mut(&mut self, view: View) -> Option<&mut dyn views::ViewPage> {
        match view {
            View::Albums => Some(&mut self.albums_page),
            View::Artists => Some(&mut self.artists_page),
            View::Songs => Some(&mut self.songs_page),
            View::Genres => Some(&mut self.genres_page),
            View::Playlists => Some(&mut self.playlists_page),
            View::Queue => Some(&mut self.queue_page),
            View::Radios => Some(&mut self.radios_page),
            View::Settings => None,
        }
    }

    /// Handle Get Info hotkey (Shift+I): open info modal for the centered item.
    /// Supports Songs, Albums (parent + child), Artists (three-tier), Playlists (parent + child), and Queue.
    pub(crate) fn handle_get_info(&mut self) -> Task<Message> {
        debug!("ℹ️ GetInfo (Shift+I) hotkey pressed");

        // Toggle: if the modal is already open, close it
        if self.info_modal.visible {
            return self.update(Message::InfoModal(
                crate::widgets::info_modal::InfoModalMessage::Close,
            ));
        }

        #[allow(clippy::collapsible_if)]
        if self.pane_focus == crate::state::PaneFocus::Browser
            && let Some(panel) = self.browsing_panel.as_ref()
            && panel.active_view == crate::views::BrowsingView::Similar
        {
            if let Some(similar) = &self.similar_songs {
                let center_idx = self
                    .similar_page
                    .common
                    .slot_list
                    .get_center_item_index(similar.songs.len());
                if let Some(song) = center_idx.and_then(|idx| similar.songs.get(idx)) {
                    let item = InfoModalItem::from_song(song);
                    return self.update(Message::InfoModal(
                        crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                    ));
                }
            }
        }
        match self.current_view {
            View::Songs => {
                let center_idx = self
                    .songs_page
                    .common
                    .slot_list
                    .get_center_item_index(self.library.songs.len());
                if let Some(song) = center_idx.and_then(|idx| self.library.songs.get(idx)) {
                    let item = InfoModalItem::from_song_view_data(song);
                    return self.update(Message::InfoModal(
                        crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                    ));
                }
            }
            View::Albums => {
                let total = self
                    .albums_page
                    .expansion
                    .flattened_len(&self.library.albums);
                let center_idx = self
                    .albums_page
                    .common
                    .slot_list
                    .get_center_item_index(total);
                if let Some(entry) = center_idx.and_then(|idx| {
                    self.albums_page
                        .expansion
                        .get_entry_at(idx, &self.library.albums, |a| &a.id)
                }) {
                    let item = match entry {
                        SlotListEntry::Child(song, _) => InfoModalItem::from_song_view_data(song),
                        SlotListEntry::Parent(album) => InfoModalItem::Album {
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
                                .albums_page
                                .expansion
                                .children
                                .first()
                                .map(|s| s.path.clone()),
                        },
                    };
                    return self.update(Message::InfoModal(
                        crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                    ));
                }
            }
            View::Artists => {
                if let Some(entry) = self.artists_page.expansion.resolve_center(
                    &self.library.artists,
                    &self.artists_page.common,
                    |a| &a.id,
                ) {
                    let item = match entry {
                        SlotListEntry::Child(album, _) => InfoModalItem::Album {
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
                        },
                        SlotListEntry::Parent(artist) => InfoModalItem::Artist {
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
                        },
                    };
                    return self.update(Message::InfoModal(
                        crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                    ));
                }
            }
            View::Playlists => {
                let total = self
                    .playlists_page
                    .expansion
                    .flattened_len(&self.library.playlists);
                let center_idx = self
                    .playlists_page
                    .common
                    .slot_list
                    .get_center_item_index(total);
                if let Some(entry) = center_idx.and_then(|idx| {
                    self.playlists_page
                        .expansion
                        .get_entry_at(idx, &self.library.playlists, |p| &p.id)
                }) {
                    let item = match entry {
                        SlotListEntry::Child(song, _) => InfoModalItem::from_song_view_data(song),
                        SlotListEntry::Parent(playlist) => InfoModalItem::Playlist {
                            name: playlist.name.clone(),
                            comment: playlist.comment.clone(),
                            duration: playlist.duration,
                            song_count: playlist.song_count,
                            size: 0,
                            owner_name: playlist.owner_name.clone(),
                            public: playlist.public,
                            created_at: String::new(),
                            updated_at: playlist.updated_at.clone(),
                            id: playlist.id.clone(),
                        },
                    };
                    return self.update(Message::InfoModal(
                        crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                    ));
                }
            }
            View::Queue => {
                // Queue uses async API re-fetch for full Song field coverage
                let filtered = self.filter_queue_songs();
                let center_idx = self
                    .queue_page
                    .common
                    .slot_list
                    .get_center_item_index(filtered.len());
                if let Some(song_id) =
                    center_idx.and_then(|idx| filtered.get(idx).map(|s| s.id.clone()))
                {
                    return self.shell_task(
                        move |shell| async move {
                            let api = shell.songs_api().await?;
                            let song = api.load_song_by_id(&song_id).await?;
                            Ok(InfoModalItem::from_song(&song))
                        },
                        |result: Result<InfoModalItem, anyhow::Error>| match result {
                            Ok(item) => Message::InfoModal(
                                crate::widgets::info_modal::InfoModalMessage::Open(Box::new(item)),
                            ),
                            Err(e) => {
                                tracing::error!("Failed to load song info: {e}");
                                Message::Toast(crate::app_message::ToastMessage::Push(
                                    nokkvi_data::types::toast::Toast::new(
                                        format!("Failed to load song info: {e}"),
                                        nokkvi_data::types::toast::ToastLevel::Error,
                                    ),
                                ))
                            }
                        },
                    );
                }
            }
            _ => {
                self.toast_info("Get Info is not available in this view");
                return Task::none();
            }
        }

        self.toast_warn("No item selected");
        Task::none()
    }

    /// Handle Shift+Enter: expand/collapse inline subgroup for center item.
    pub(crate) fn handle_expand_center(&mut self) -> Task<Message> {
        trace!(" ExpandCenter (Shift+Enter) hotkey pressed");
        // Settings uses drill-down navigation, not inline expand/collapse
        if self.current_view == crate::View::Settings {
            return Task::none();
        }
        if let Some(msg) = self
            .current_view_page()
            .and_then(|p| p.expand_center_message())
        {
            return Task::done(msg);
        }
        Task::none()
    }

    /// Dispatch a `HotkeyMessage` to its handler.
    ///
    /// `ClearSearch` runs inline modal-close logic before delegating, since
    /// Escape's job-cascade (close modal first, then clear search) is part
    /// of the dispatch decision rather than belonging to a single handler.
    pub(super) fn dispatch_hotkey(&mut self, msg: HotkeyMessage) -> Task<Message> {
        match msg {
            HotkeyMessage::ClearSearch => {
                // If EQ modal is visible, Escape closes it first
                if self.window.eq_modal_open {
                    self.window.eq_modal_open = false;
                    return Task::none();
                }
                // If about modal is visible, Escape closes it first
                if self.about_modal.visible {
                    self.about_modal.close();
                    return Task::none();
                }
                // If info modal is visible, Escape closes it first
                if self.info_modal.visible {
                    self.info_modal.close();
                    return Task::none();
                }
                self.handle_clear_search()
            }
            HotkeyMessage::CycleSortMode(forward) => self.handle_cycle_sort_mode(forward),
            HotkeyMessage::CenterOnPlaying => self.handle_center_on_playing(),
            HotkeyMessage::ToggleStar => self.handle_toggle_star(),
            HotkeyMessage::SongStarredStatusUpdated(song_id, new_starred_status) => {
                self.handle_song_starred_status_updated(song_id, new_starred_status)
            }
            HotkeyMessage::AlbumStarredStatusUpdated(album_id, new_starred_status) => {
                self.handle_album_starred_status_updated(album_id, new_starred_status)
            }
            HotkeyMessage::ArtistStarredStatusUpdated(artist_id, new_starred_status) => {
                self.handle_artist_starred_status_updated(artist_id, new_starred_status)
            }
            HotkeyMessage::AddToQueue => self.handle_add_to_queue(),
            HotkeyMessage::SaveQueueAsPlaylist => self.handle_save_queue_as_playlist(),
            HotkeyMessage::RemoveFromQueue => self.handle_remove_from_queue(),
            HotkeyMessage::ClearQueue => self.handle_clear_queue(),
            HotkeyMessage::FocusSearch => self.handle_focus_search(),
            HotkeyMessage::IncreaseRating => self.handle_increase_rating(),
            HotkeyMessage::DecreaseRating => self.handle_decrease_rating(),
            HotkeyMessage::SongRatingUpdated(song_id, new_rating) => {
                self.handle_song_rating_updated(song_id, new_rating)
            }
            HotkeyMessage::SongPlayCountIncremented(song_id) => {
                self.handle_song_play_count_incremented(song_id)
            }
            HotkeyMessage::AlbumRatingUpdated(album_id, new_rating) => {
                self.handle_album_rating_updated(album_id, new_rating)
            }
            HotkeyMessage::ArtistRatingUpdated(artist_id, new_rating) => {
                self.handle_artist_rating_updated(artist_id, new_rating)
            }
            HotkeyMessage::ExpandCenter => self.handle_expand_center(),
            HotkeyMessage::MoveTrackUp => self.handle_move_track(true),
            HotkeyMessage::MoveTrackDown => self.handle_move_track(false),
            HotkeyMessage::GetInfo => self.handle_get_info(),
            HotkeyMessage::FindSimilar => self.handle_find_similar_for_playing_track(),
            HotkeyMessage::FindTopSongs => self.handle_find_top_songs_for_playing_track(),
            HotkeyMessage::EditValue(up) => self.handle_edit_value(up),
            HotkeyMessage::RefreshView => match self.current_view {
                crate::View::Albums => Task::done(Message::LoadAlbums),
                crate::View::Artists => Task::done(Message::LoadArtists),
                crate::View::Songs => Task::done(Message::LoadSongs),
                crate::View::Genres => Task::done(Message::LoadGenres),
                crate::View::Playlists => Task::done(Message::LoadPlaylists),
                crate::View::Radios => Task::done(Message::LoadRadioStations),
                crate::View::Queue | crate::View::Settings => Task::none(),
            },
            HotkeyMessage::StartRoulette => Task::done(Message::Roulette(
                crate::app_message::RouletteMessage::Start(self.current_view),
            )),
        }
    }
}
