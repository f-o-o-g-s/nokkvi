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
    app_message::Message,
    views,
    views::expansion::{self, SlotListEntry, ThreeTierEntry},
};

impl Nokkvi {
    /// Get the current view as a `&dyn ViewPage` for trait-based dispatch.
    /// Returns None for Settings (which doesn't implement ViewPage).
    ///
    /// In playlist edit mode with browser focus, returns the browsing panel's
    /// active view page so all existing hotkey handlers work on the browser pane.
    pub(crate) fn current_view_page(&self) -> Option<&dyn views::ViewPage> {
        // Pane-aware routing: when editing with browser focus, delegate to the active tab
        if self.playlist_edit.is_some()
            && self.pane_focus == crate::state::PaneFocus::Browser
            && let Some(panel) = &self.browsing_panel
        {
            return match panel.active_view {
                views::BrowsingView::Albums => Some(&self.albums_page),
                views::BrowsingView::Songs => Some(&self.songs_page),
                views::BrowsingView::Artists => Some(&self.artists_page),
                views::BrowsingView::Genres => Some(&self.genres_page),
            };
        }

        match self.current_view {
            View::Albums => Some(&self.albums_page),
            View::Artists => Some(&self.artists_page),
            View::Songs => Some(&self.songs_page),
            View::Genres => Some(&self.genres_page),
            View::Playlists => Some(&self.playlists_page),
            View::Queue => Some(&self.queue_page),
            View::Settings => None,
        }
    }

    /// Get the current view as a `&mut dyn ViewPage` for trait-based dispatch.
    /// Returns None for Settings (which doesn't implement ViewPage).
    ///
    /// In playlist edit mode with browser focus, returns the browsing panel's
    /// active view page so all existing hotkey handlers work on the browser pane.
    pub(crate) fn current_view_page_mut(&mut self) -> Option<&mut dyn views::ViewPage> {
        // Pane-aware routing: when editing with browser focus, delegate to the active tab
        if self.playlist_edit.is_some()
            && self.pane_focus == crate::state::PaneFocus::Browser
            && let Some(panel) = &self.browsing_panel
        {
            return match panel.active_view {
                views::BrowsingView::Albums => Some(&mut self.albums_page),
                views::BrowsingView::Songs => Some(&mut self.songs_page),
                views::BrowsingView::Artists => Some(&mut self.artists_page),
                views::BrowsingView::Genres => Some(&mut self.genres_page),
            };
        }

        match self.current_view {
            View::Albums => Some(&mut self.albums_page),
            View::Artists => Some(&mut self.artists_page),
            View::Songs => Some(&mut self.songs_page),
            View::Genres => Some(&mut self.genres_page),
            View::Playlists => Some(&mut self.playlists_page),
            View::Queue => Some(&mut self.queue_page),
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
                let total = expansion::three_tier_flattened_len(
                    &self.library.artists,
                    &self.artists_page.expansion,
                    self.artists_page.sub_expansion.children.len(),
                );
                let center_idx = self
                    .artists_page
                    .common
                    .slot_list
                    .get_center_item_index(total);
                if let Some(entry) = center_idx.and_then(|idx| {
                    expansion::three_tier_get_entry_at(
                        idx,
                        &self.library.artists,
                        &self.artists_page.expansion,
                        &self.artists_page.sub_expansion,
                        |a| &a.id,
                        |a| &a.id,
                    )
                }) {
                    let item = match entry {
                        ThreeTierEntry::Grandchild(song, _) => {
                            InfoModalItem::from_song_view_data(song)
                        }
                        ThreeTierEntry::Child(album, _) => InfoModalItem::Album {
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
                                .artists_page
                                .sub_expansion
                                .children
                                .first()
                                .map(|s| s.path.clone()),
                        },
                        ThreeTierEntry::Parent(artist) => InfoModalItem::Artist {
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
}
