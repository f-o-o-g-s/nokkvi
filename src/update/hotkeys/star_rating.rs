//! Star/Favorite and Rating hotkey handlers

use iced::Task;
use tracing::{debug, error, info};

use crate::{
    Nokkvi, View,
    app_message::Message,
    views::expansion::{self, SlotListEntry, ThreeTierEntry},
};

/// Info about the currently centered item in the slot list, used by star/rating hotkeys.
pub(in crate::update) struct CenterItemInfo {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub starred: bool,
    pub rating: u32,
    pub item_type: &'static str,
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
                        item_type: "song",
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
                        item_type: "song",
                    },
                    SlotListEntry::Parent(album) => CenterItemInfo {
                        id: album.id.clone(),
                        name: album.name.clone(),
                        artist: album.artist.clone(),
                        starred: album.is_starred,
                        rating: album.rating.unwrap_or(0),
                        item_type: "album",
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
                    item_type: "song",
                }),
            View::Artists => expansion::resolve_three_tier_center(
                &self.library.artists,
                &self.artists_page.expansion,
                &self.artists_page.sub_expansion,
                &self.artists_page.common,
                |a| &a.id,
                |a| &a.id,
            )
            .map(|entry| match entry {
                ThreeTierEntry::Grandchild(song, _) => CenterItemInfo {
                    id: song.id.clone(),
                    name: song.title.clone(),
                    artist: song.artist.clone(),
                    starred: song.is_starred,
                    rating: song.rating.unwrap_or(0),
                    item_type: "song",
                },
                ThreeTierEntry::Child(album, _) => CenterItemInfo {
                    id: album.id.clone(),
                    name: album.name.clone(),
                    artist: album.artist.clone(),
                    starred: album.is_starred,
                    rating: album.rating.unwrap_or(0),
                    item_type: "album",
                },
                ThreeTierEntry::Parent(artist) => CenterItemInfo {
                    id: artist.id.clone(),
                    name: artist.name.clone(),
                    artist: String::new(),
                    starred: artist.is_starred,
                    rating: artist.rating.unwrap_or(0),
                    item_type: "artist",
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
                        item_type: "song",
                    }),
                    SlotListEntry::Parent(_) => None, // playlists themselves can't be starred/rated
                }),
            View::Genres => expansion::resolve_three_tier_center(
                &self.library.genres,
                &self.genres_page.expansion,
                &self.genres_page.sub_expansion,
                &self.genres_page.common,
                |g| &g.id,
                |a| &a.id,
            )
            .and_then(|entry| match entry {
                ThreeTierEntry::Grandchild(song, _) => Some(CenterItemInfo {
                    id: song.id.clone(),
                    name: song.title.clone(),
                    artist: song.artist.clone(),
                    starred: song.is_starred,
                    rating: song.rating.unwrap_or(0),
                    item_type: "song",
                }),
                ThreeTierEntry::Child(album, _) => Some(CenterItemInfo {
                    id: album.id.clone(),
                    name: album.name.clone(),
                    artist: album.artist.clone(),
                    starred: album.is_starred,
                    rating: album.rating.unwrap_or(0),
                    item_type: "album",
                }),
                ThreeTierEntry::Parent(_) => None, // genres themselves can't be starred/rated
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
            info.item_type,
            info.name,
            info.artist,
            if info.starred {
                "starred"
            } else {
                "not starred"
            }
        );

        // Apply optimistic update immediately
        let optimistic_msg =
            Self::starred_revert_message(info.id.clone(), info.item_type, new_starred);

        let item_type_owned = info.item_type.to_string();
        let revert_id = info.id.clone();
        let revert_type = info.item_type.to_string();
        let current_starred = info.starred;
        let toast_name = info.name.clone();
        let name = info.name;
        let artist = info.artist;
        let item_id = info.id;

        let api_task = self.shell_task(
            move |shell| async move {
                let auth_vm = shell.auth().clone();
                let client = match auth_vm.get_client().await {
                    Some(client) => client,
                    None => return Err(anyhow::anyhow!("No API client available")),
                };

                let server_url = auth_vm.get_server_url().await;
                let subsonic_credential = auth_vm.get_subsonic_credential().await;

                nokkvi_data::services::api::star::toggle_star(
                    &client.http_client(),
                    &server_url,
                    &subsonic_credential,
                    &item_id,
                    &item_type_owned,
                    current_starred,
                )
                .await?;

                debug!(
                    "✅ {} {}: {} - {}",
                    if new_starred { "Starred" } else { "Unstarred" },
                    item_type_owned,
                    name,
                    artist
                );

                Ok::<_, anyhow::Error>(())
            },
            move |result| match result {
                Ok(()) => {
                    let label = if new_starred {
                        "★ Starred"
                    } else {
                        "☆ Unstarred"
                    };
                    let msg = format!("{label}: {toast_name}");
                    Message::Toast(crate::app_message::ToastMessage::Push(
                        nokkvi_data::types::toast::Toast::new(
                            msg,
                            nokkvi_data::types::toast::ToastLevel::Success,
                        ),
                    ))
                }
                Err(e) => {
                    error!(" Failed to toggle star: {}", e);
                    // Revert to original starred state
                    Self::starred_revert_message(revert_id, &revert_type, current_starred)
                }
            },
        );

        // Batch: apply optimistic update + fire API call
        Task::batch(vec![Task::done(optimistic_msg), api_task])
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
        // Update sub-expanded tracks in artists and genres views
        if let Some(track) = self
            .artists_page
            .sub_expansion
            .children
            .iter_mut()
            .find(|t| t.id == song_id)
        {
            track.is_starred = new_starred_status;
        }
        if let Some(track) = self
            .genres_page
            .sub_expansion
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
                    nokkvi_data::services::api::rating::set_rating(
                        &client.http_client(),
                        &auth_vm.get_server_url().await,
                        &auth_vm.get_subsonic_credential().await,
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
            info.item_type, display_name, current_rating, new_rating
        );

        // Apply optimistic update immediately
        let optimistic_msg =
            Self::rating_revert_message(info.id.clone(), info.item_type, new_rating);

        let item_type_owned = info.item_type.to_string();
        let revert_id = info.id.clone();
        let revert_type = info.item_type.to_string();
        let item_id = info.id;
        let toast_display_name = display_name.clone();

        let api_task = self.shell_task(
            move |shell| async move {
                let auth_vm = shell.auth().clone();
                let client = match auth_vm.get_client().await {
                    Some(client) => client,
                    None => return Err(anyhow::anyhow!("No API client available")),
                };

                let server_url = auth_vm.get_server_url().await;
                let subsonic_credential = auth_vm.get_subsonic_credential().await;

                nokkvi_data::services::api::rating::set_rating(
                    &client.http_client(),
                    &server_url,
                    &subsonic_credential,
                    &item_id,
                    new_rating,
                )
                .await?;

                info!(
                    "⭐ Set rating for {} {}: {}",
                    item_type_owned, display_name, new_rating
                );

                Ok::<_, anyhow::Error>(())
            },
            move |result| match result {
                Ok(()) => Message::Toast(crate::app_message::ToastMessage::Push(
                    nokkvi_data::types::toast::Toast::new(
                        format!("⭐ Rated {toast_display_name}: {new_rating}/5"),
                        nokkvi_data::types::toast::ToastLevel::Success,
                    ),
                )),
                Err(e) => {
                    error!(" Failed to set rating: {}", e);
                    // Revert to original rating
                    Self::rating_revert_message(revert_id, &revert_type, current_rating)
                }
            },
        );

        // Batch: apply optimistic update + fire API call
        Task::batch(vec![Task::done(optimistic_msg), api_task])
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
        // Also update sub-expanded tracks in artists/genres views
        if let Some(track) = self
            .artists_page
            .sub_expansion
            .children
            .iter_mut()
            .find(|t| t.id == song_id)
        {
            track.rating = rating_opt;
        }
        if let Some(track) = self
            .genres_page
            .sub_expansion
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
        nokkvi_data::backend::increment_play_count_in_list(
            &mut self.artists_page.sub_expansion.children,
            &song_id,
            "artist track",
        );
        nokkvi_data::backend::increment_play_count_in_list(
            &mut self.genres_page.sub_expansion.children,
            &song_id,
            "genre track",
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
