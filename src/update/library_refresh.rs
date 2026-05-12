//! Handles Navidrome event-driven library refresh
use std::collections::HashSet;

use iced::Task;
use tracing::info;

use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, Message},
    services::navidrome_sse::LibraryChange,
    widgets::view_header::SortMode,
};

impl Nokkvi {
    pub(crate) fn handle_library_changed(&mut self, change: LibraryChange) -> Task<Message> {
        let LibraryChange {
            album_ids,
            artist_ids,
            song_ids,
            playlist_ids,
            genre_ids,
            is_wildcard,
        } = change;

        info!(
            "🔄 Navidrome library changed (wildcard={is_wildcard}, albums={}, artists={}, songs={}, playlists={}, genres={}), initiating background refresh",
            album_ids.len(),
            artist_ids.len(),
            song_ids.len(),
            playlist_ids.len(),
            genre_ids.len(),
        );

        let mut tasks = Vec::new();

        // Each branch fires only when the SSE payload flagged that entity kind
        // (or signalled a wildcard / full-scan). The buffer-non-empty gate
        // skips views the user hasn't visited yet — those will fetch fresh
        // on first visit. The Random-sort gate protects the artwork
        // reference (a background reload would return a new random order and
        // jar the user mid-browse); the user can press F5 to re-randomize
        // intentionally.
        let affects_albums = is_wildcard || !album_ids.is_empty();
        let affects_artists = is_wildcard || !artist_ids.is_empty();
        let affects_songs = is_wildcard || !song_ids.is_empty();
        let affects_playlists = is_wildcard || !playlist_ids.is_empty();
        let affects_genres = is_wildcard || !genre_ids.is_empty();

        // 1. Snapshot current viewport state and trigger reload for Albums.
        if affects_albums
            && !self.library.albums.is_empty()
            && self.albums_page.common.current_sort_mode != SortMode::Random
        {
            let offset = self.albums_page.common.slot_list.viewport_offset;
            let anchor_id = self.library.albums.get(offset).map(|a| a.id.clone());
            tasks.push(self.handle_load_albums(true, anchor_id));
        }

        // 2. Snapshot current viewport state and trigger reload for Artists.
        if affects_artists
            && !self.library.artists.is_empty()
            && self.artists_page.common.current_sort_mode != SortMode::Random
        {
            let offset = self.artists_page.common.slot_list.viewport_offset;
            let anchor_id = self.library.artists.get(offset).map(|a| a.id.clone());
            tasks.push(self.handle_load_artists(true, anchor_id));
        }

        // 3. Snapshot current viewport state and trigger reload for Songs.
        if affects_songs
            && !self.library.songs.is_empty()
            && self.songs_page.common.current_sort_mode != SortMode::Random
        {
            let offset = self.songs_page.common.slot_list.viewport_offset;
            let anchor_id = self.library.songs.get(offset).map(|a| a.id.clone());
            tasks.push(self.handle_load_songs(true, anchor_id));
        }

        // 4. Playlists: single-shot reload (not paged, no anchor-based
        //    re-positioning needed).
        if affects_playlists && !self.library.playlists.is_empty() {
            tasks.push(self.handle_load_playlists());
        }

        // 5. Genres: single-shot reload, same shape as playlists.
        if affects_genres && !self.library.genres.is_empty() {
            tasks.push(self.handle_load_genres());
        }

        // Notify the user gently (skipped when the user has opted to suppress
        // these notifications via Settings → General → Application).
        if !self.suppress_library_refresh_toasts {
            self.toast_info("Library refreshed automatically");
        }

        // 6. On non-wildcard events, surgically refresh artwork for the changed
        //    albums in any in-RAM Handle map. With no client-side disk cache,
        //    "refresh" here means: re-fetch from server and replace the Handle
        //    so Iced's GPU texture cache picks up the new bytes. Albums not
        //    present in any UI map will simply re-fetch on next viewport entry.
        //    Wildcards (full-library scans) skip this — per `gotchas.md`, we
        //    don't want a silent re-download of every cover.
        if !is_wildcard && !album_ids.is_empty() {
            let unique: Vec<String> = album_ids
                .into_iter()
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();

            let mut refreshed_in_ui = 0usize;
            for id in unique {
                let in_ui = self.artwork.large_artwork.peek(&id).is_some()
                    || self.artwork.album_art.contains(&id);

                if in_ui {
                    refreshed_in_ui += 1;
                    tasks.push(Task::done(Message::Artwork(
                        ArtworkMessage::RefreshAlbumArtworkSilent(id),
                    )));
                }
            }

            if refreshed_in_ui > 0 && !self.suppress_library_refresh_toasts {
                let suffix = if refreshed_in_ui == 1 { "" } else { "s" };
                self.toast_info(format!(
                    "Updated artwork for {refreshed_in_ui} album{suffix}"
                ));
            }
        }

        Task::batch(tasks)
    }
}
