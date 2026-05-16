//! Star/Favorite and Rating hotkey handlers

use iced::Task;
use nokkvi_data::types::ItemKind;
use tracing::{debug, error};

use crate::{Nokkvi, View, app_message::Message, views::expansion::SlotListEntry};

/// Info about the currently centered item in the slot list, used by star/rating hotkeys.
pub(in crate::update) struct CenterItemInfo {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub starred: bool,
    pub rating: u32,
    pub kind: ItemKind,
}

impl Nokkvi {
    /// Extract info about the currently centered item in the slot list.
    /// Shared by toggle_star and rating_change hotkey handlers.
    pub(in crate::update) fn get_center_item_info(&self) -> Option<CenterItemInfo> {
        match self.current_view {
            View::Queue => {
                let filtered = self.filter_queue_songs();
                self.queue_page
                    .common
                    .slot_list
                    .get_center_item_index(filtered.len())
                    .and_then(|idx| filtered.get(idx).cloned())
                    .map(|song| CenterItemInfo {
                        id: song.id.clone(),
                        name: song.title.clone(),
                        artist: song.artist.clone(),
                        starred: song.starred,
                        rating: song.rating.unwrap_or(0),
                        kind: ItemKind::Song,
                    })
            }
            View::Albums => self
                .albums_page
                .expansion
                .resolve_center(&self.library.albums, &self.albums_page.common, |a| &a.id)
                .map(|entry| match entry {
                    SlotListEntry::Child(song, _) => CenterItemInfo {
                        id: song.id.clone(),
                        name: song.title.clone(),
                        artist: song.artist.clone(),
                        starred: song.is_starred,
                        rating: song.rating.unwrap_or(0),
                        kind: ItemKind::Song,
                    },
                    SlotListEntry::Parent(album) => CenterItemInfo {
                        id: album.id.clone(),
                        name: album.name.clone(),
                        artist: album.artist.clone(),
                        starred: album.is_starred,
                        rating: album.rating.unwrap_or(0),
                        kind: ItemKind::Album,
                    },
                }),
            View::Songs => self
                .songs_page
                .common
                .slot_list
                .get_center_item_index(self.library.songs.len())
                .and_then(|idx| self.library.songs.get(idx))
                .map(|song| CenterItemInfo {
                    id: song.id.clone(),
                    name: song.title.clone(),
                    artist: song.artist.clone(),
                    starred: song.is_starred,
                    rating: song.rating.unwrap_or(0),
                    kind: ItemKind::Song,
                }),
            View::Artists => self
                .artists_page
                .expansion
                .resolve_center(&self.library.artists, &self.artists_page.common, |a| &a.id)
                .map(|entry| match entry {
                    SlotListEntry::Child(album, _) => CenterItemInfo {
                        id: album.id.clone(),
                        name: album.name.clone(),
                        artist: album.artist.clone(),
                        starred: album.is_starred,
                        rating: album.rating.unwrap_or(0),
                        kind: ItemKind::Album,
                    },
                    SlotListEntry::Parent(artist) => CenterItemInfo {
                        id: artist.id.clone(),
                        name: artist.name.clone(),
                        artist: String::new(),
                        starred: artist.is_starred,
                        rating: artist.rating.unwrap_or(0),
                        kind: ItemKind::Artist,
                    },
                }),
            View::Playlists => self
                .playlists_page
                .expansion
                .resolve_center(&self.library.playlists, &self.playlists_page.common, |p| {
                    &p.id
                })
                .and_then(|entry| match entry {
                    SlotListEntry::Child(song, _) => Some(CenterItemInfo {
                        id: song.id.clone(),
                        name: song.title.clone(),
                        artist: song.artist.clone(),
                        starred: song.is_starred,
                        rating: song.rating.unwrap_or(0),
                        kind: ItemKind::Song,
                    }),
                    SlotListEntry::Parent(_) => None, // playlists themselves can't be starred/rated
                }),
            View::Genres => self
                .genres_page
                .expansion
                .resolve_center(&self.library.genres, &self.genres_page.common, |g| &g.id)
                .and_then(|entry| match entry {
                    SlotListEntry::Child(album, _) => Some(CenterItemInfo {
                        id: album.id.clone(),
                        name: album.name.clone(),
                        artist: album.artist.clone(),
                        starred: album.is_starred,
                        rating: album.rating.unwrap_or(0),
                        kind: ItemKind::Album,
                    }),
                    SlotListEntry::Parent(_) => None, // genres themselves can't be starred/rated
                }),
            _ => None,
        }
    }

    pub(crate) fn handle_toggle_star(&mut self) -> Task<Message> {
        debug!(" ToggleStar (Shift+L) hotkey pressed");

        let Some(info) = self.get_center_item_info() else {
            self.toast_warn("No item selected");
            return Task::none();
        };

        let new_starred = !info.starred;
        debug!(
            "  Toggling star for {}: {} - {} (currently: {})",
            info.kind,
            info.name,
            info.artist,
            if info.starred {
                "starred"
            } else {
                "not starred"
            }
        );

        let label = if new_starred {
            "★ Starred"
        } else {
            "☆ Unstarred"
        };
        self.toast_success(format!("{label}: {}", info.name));

        self.toggle_star_with_revert_task(info.id, info.kind, new_starred)
    }

    pub(crate) fn handle_song_starred_status_updated(
        &mut self,
        song_id: String,
        new_starred_status: bool,
    ) -> Task<Message> {
        debug!(
            "🔄 Updating starred status for song {} to {}",
            song_id, new_starred_status
        );
        // Update all lists that may contain this song
        nokkvi_data::backend::update_starred_in_list(
            &mut self.library.songs,
            &song_id,
            new_starred_status,
            "songs",
        );
        nokkvi_data::backend::update_starred_in_list(
            &mut self.library.queue_songs,
            &song_id,
            new_starred_status,
            "queue",
        );
        if let Some(similar) = &mut self.similar_songs {
            nokkvi_data::backend::update_starred_in_list(
                &mut similar.songs,
                &song_id,
                new_starred_status,
                "similar",
            );
        }
        // Update expanded children in any view that shows tracks
        if let Some(track) = self
            .albums_page
            .expansion
            .children
            .iter_mut()
            .find(|t| t.id == song_id)
        {
            track.is_starred = new_starred_status;
        }
        if let Some(track) = self
            .playlists_page
            .expansion
            .children
            .iter_mut()
            .find(|t| t.id == song_id)
        {
            track.is_starred = new_starred_status;
        }

        let mut tasks: Vec<Task<Message>> = Vec::new();

        // Loving a song → also rate it 5 stars (local + API)
        if new_starred_status {
            tasks.push(Task::done(Message::Hotkey(
                crate::app_message::HotkeyMessage::SongRatingUpdated(song_id.clone(), 5),
            )));
            let sid = song_id.clone();
            tasks.push(self.shell_task(
                move |shell| async move {
                    let auth_vm = shell.auth().clone();
                    let client = auth_vm
                        .get_client()
                        .await
                        .ok_or_else(|| anyhow::anyhow!("No API client available"))?;
                    let (server_url, subsonic_credential) = auth_vm.server_config().await;
                    nokkvi_data::services::api::rating::set_rating(
                        &client.http_client(),
                        &server_url,
                        &subsonic_credential,
                        &sid,
                        5,
                    )
                    .await
                },
                |result| {
                    if let Err(e) = result {
                        error!("⭐ Failed to set 5-star rating on loved song: {}", e);
                    }
                    Message::NoOp
                },
            ));
        }

        // Persist to queue storage
        let sid = song_id.clone();
        tasks.push(self.shell_task(
            move |shell| async move {
                let queue_manager = shell.queue().queue_manager();
                let mut qm = queue_manager.lock().await;
                qm.update_song_starred(&sid, new_starred_status).ok();
            },
            |_| Message::NoOp,
        ));

        if tasks.is_empty() {
            Task::none()
        } else {
            Task::batch(tasks)
        }
    }

    pub(crate) fn handle_album_starred_status_updated(
        &mut self,
        album_id: String,
        new_starred_status: bool,
    ) -> Task<Message> {
        debug!(
            "🔄 Updating starred status for album {} to {}",
            album_id, new_starred_status
        );
        nokkvi_data::backend::update_starred_in_list(
            &mut self.library.albums,
            &album_id,
            new_starred_status,
            "albums",
        );
        // Update expanded child albums in artists/genres views
        if let Some(album) = self
            .artists_page
            .expansion
            .children
            .iter_mut()
            .find(|a| a.id == album_id)
        {
            album.is_starred = new_starred_status;
        }
        if let Some(album) = self
            .genres_page
            .expansion
            .children
            .iter_mut()
            .find(|a| a.id == album_id)
        {
            album.is_starred = new_starred_status;
        }
        Task::none()
    }

    pub(crate) fn handle_artist_starred_status_updated(
        &mut self,
        artist_id: String,
        new_starred_status: bool,
    ) -> Task<Message> {
        debug!(
            "🔄 Updating starred status for artist {} to {}",
            artist_id, new_starred_status
        );
        nokkvi_data::backend::update_starred_in_list(
            &mut self.library.artists,
            &artist_id,
            new_starred_status,
            "artists",
        );
        Task::none()
    }

    /// Handle increasing the rating of the currently centered item
    pub(crate) fn handle_increase_rating(&mut self) -> Task<Message> {
        self.handle_rating_change(true)
    }

    /// Handle decreasing the rating of the currently centered item
    pub(crate) fn handle_decrease_rating(&mut self) -> Task<Message> {
        self.handle_rating_change(false)
    }

    /// Shared logic for rating increase/decrease
    fn handle_rating_change(&mut self, increase: bool) -> Task<Message> {
        let direction = if increase { "Increase" } else { "Decrease" };
        debug!(" {} rating hotkey pressed", direction);

        let Some(info) = self.get_center_item_info() else {
            self.toast_warn("No item selected");
            return Task::none();
        };

        let current_rating = info.rating;
        let new_rating = if increase {
            (current_rating + 1).min(5)
        } else {
            current_rating.saturating_sub(1)
        };

        // Skip if no change (already at boundary)
        let display_name = if info.artist.is_empty() {
            info.name.clone()
        } else {
            format!("{} - {}", info.name, info.artist)
        };
        if new_rating == current_rating {
            debug!(
                "  Rating already at {} for {}, skipping",
                current_rating, display_name
            );
            self.toast_info(format!(
                "Rating already at {current_rating}/5 for {display_name}"
            ));
            return Task::none();
        }

        debug!(
            "  Setting rating for {} {}: {} -> {}",
            info.kind, display_name, current_rating, new_rating
        );

        self.toast_success(format!("⭐ Rated {display_name}: {new_rating}/5"));

        self.set_item_rating_task(info.id, info.kind, new_rating as usize, current_rating)
    }

    /// Update song rating in local state after successful API call
    pub(crate) fn handle_song_rating_updated(
        &mut self,
        song_id: String,
        new_rating: u32,
    ) -> Task<Message> {
        debug!("🔄 Updating rating for song {} to {}", song_id, new_rating);
        let rating_opt = if new_rating == 0 {
            None
        } else {
            Some(new_rating)
        };
        nokkvi_data::backend::update_rating_in_list(
            &mut self.library.queue_songs,
            &song_id,
            rating_opt,
            "queue song",
        );
        nokkvi_data::backend::update_rating_in_list(
            &mut self.library.songs,
            &song_id,
            rating_opt,
            "song",
        );
        if let Some(similar) = &mut self.similar_songs {
            nokkvi_data::backend::update_rating_in_list(
                &mut similar.songs,
                &song_id,
                rating_opt,
                "similar",
            );
        }
        // Also update expanded tracks in all views that show songs
        if let Some(track) = self
            .albums_page
            .expansion
            .children
            .iter_mut()
            .find(|t| t.id == song_id)
        {
            track.rating = rating_opt;
        }
        if let Some(track) = self
            .playlists_page
            .expansion
            .children
            .iter_mut()
            .find(|t| t.id == song_id)
        {
            track.rating = rating_opt;
        }
        // Persist to queue storage
        let sid = song_id.clone();
        self.shell_task(
            move |shell| async move {
                let queue_manager = shell.queue().queue_manager();
                let mut qm = queue_manager.lock().await;
                qm.update_song_rating(&sid, rating_opt).ok();
            },
            |_| Message::NoOp,
        )
    }

    /// Bump a song's play count by 1 across every in-memory song collection,
    /// then persist the new count to the queue manager. Dispatched from
    /// `handle_scrobble_submission_result` on a successful submission so the
    /// UI mirrors what Navidrome just incremented server-side.
    pub(crate) fn handle_song_play_count_incremented(&mut self, song_id: String) -> Task<Message> {
        debug!("🔁 Bumping play count for song {}", song_id);
        nokkvi_data::backend::increment_play_count_in_list(
            &mut self.library.queue_songs,
            &song_id,
            "queue song",
        );
        nokkvi_data::backend::increment_play_count_in_list(
            &mut self.library.songs,
            &song_id,
            "song",
        );
        if let Some(similar) = &mut self.similar_songs {
            nokkvi_data::backend::increment_play_count_in_list(
                &mut similar.songs,
                &song_id,
                "similar",
            );
        }
        nokkvi_data::backend::increment_play_count_in_list(
            &mut self.albums_page.expansion.children,
            &song_id,
            "album track",
        );
        nokkvi_data::backend::increment_play_count_in_list(
            &mut self.playlists_page.expansion.children,
            &song_id,
            "playlist track",
        );

        let sid = song_id.clone();
        self.shell_task(
            move |shell| async move {
                let queue_manager = shell.queue().queue_manager();
                let mut qm = queue_manager.lock().await;
                qm.increment_song_play_count(&sid).ok();
            },
            |_| Message::NoOp,
        )
    }

    /// Update album rating in local state after successful API call
    pub(crate) fn handle_album_rating_updated(
        &mut self,
        album_id: String,
        new_rating: u32,
    ) -> Task<Message> {
        debug!(
            "🔄 Updating rating for album {} to {}",
            album_id, new_rating
        );
        let rating_opt = if new_rating == 0 {
            None
        } else {
            Some(new_rating)
        };
        nokkvi_data::backend::update_rating_in_list(
            &mut self.library.albums,
            &album_id,
            rating_opt,
            "album",
        );
        // Update expanded child albums in artists/genres views
        if let Some(album) = self
            .artists_page
            .expansion
            .children
            .iter_mut()
            .find(|a| a.id == album_id)
        {
            album.rating = rating_opt;
        }
        if let Some(album) = self
            .genres_page
            .expansion
            .children
            .iter_mut()
            .find(|a| a.id == album_id)
        {
            album.rating = rating_opt;
        }
        Task::none()
    }

    /// Update artist rating in local state after successful API call
    pub(crate) fn handle_artist_rating_updated(
        &mut self,
        artist_id: String,
        new_rating: u32,
    ) -> Task<Message> {
        debug!(
            "🔄 Updating rating for artist {} to {}",
            artist_id, new_rating
        );
        let rating_opt = if new_rating == 0 {
            None
        } else {
            Some(new_rating)
        };
        nokkvi_data::backend::update_rating_in_list(
            &mut self.library.artists,
            &artist_id,
            rating_opt,
            "artist",
        );
        Task::none()
    }
}
