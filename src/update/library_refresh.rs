//! Navidrome SSE-driven library refresh.
//!
//! Reacts to server-side `library_changed` events on the Navidrome SSE
//! stream by reloading affected browse buffers (Albums / Artists /
//! Songs / Playlists / Genres) and, on wildcard full-scan events, also
//! refreshing the multi-library list itself. Sibling file
//! [`super::library_filter`] handles the user-initiated nav-bar
//! popover open / toggle / explicit-refresh code path.
use iced::Task;
use tracing::info;

use crate::{
    Nokkvi, app_message::Message, services::navidrome_sse::LibraryChange,
    widgets::view_header::SortMode,
};

impl Nokkvi {
    pub(crate) fn handle_library_changed(&mut self, change: LibraryChange) -> Task<Message> {
        // Skip the reload while the user is mid-gesture: a server-pushed
        // refreshResource arriving during a playlist edit or cross-pane drag
        // would reset scroll / selection / viewport under the in-progress
        // interaction. The edit buffer and drag snapshot are decoupled state
        // that survive, so the next post-gesture SSE event (or the manual
        // Refresh button) reconciles the library. Placed before the toast
        // below so no misleading "Library refreshed" message fires either.
        if self.playlist_editor.is_some() || self.cross_pane_drag.active.is_some() {
            tracing::debug!(
                " [SSE] LibraryChanged received during active edit/drag; skipping reload"
            );
            return Task::none();
        }

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

        // 6. Library list refresh on wildcard (full-scan) events. Navidrome
        //    does not emit a dedicated `library_changed` SSE event today (see
        //    plan §14.3), so the popover's source-of-truth would otherwise
        //    drift if an admin adds / renames / removes a library while the
        //    client is running. Re-fetch on every wildcard refresh so the
        //    next time the user opens the popover, the row count matches
        //    the server. `Library::Loaded` also prunes `active_library_ids`
        //    of any IDs no longer in the refreshed list.
        if is_wildcard {
            tasks.push(self.shell_task(
                |shell| async move { shell.refresh_libraries().await },
                |result: anyhow::Result<Vec<nokkvi_data::types::library::Library>>| match result {
                    Ok(libs) => Message::Library(crate::app_message::LibraryMessage::Loaded(libs)),
                    Err(e) => {
                        if let Some(msg) = crate::update::components::session_expired_message(&e) {
                            return msg;
                        }
                        Message::Library(crate::app_message::LibraryMessage::LoadFailed(format!(
                            "{e:#}"
                        )))
                    }
                },
            ));
        }

        // Notify the user gently (skipped when the user has opted to suppress
        // these notifications via Settings → General → Application).
        if !self.settings.suppress_library_refresh_toasts {
            self.toast_info("Library refreshed automatically");
        }

        // Artwork refresh is NOT triggered from SSE library-changed events.
        // Navidrome fires `library_changed` on every play-count bump, so an
        // auto-refresh produced a one-frame GPU-upload flicker on the large
        // artwork panel after every scrobble — see `update/albums.rs` for the
        // `Handle::from_bytes` cache-busting mechanic. Cover-art replacements
        // propagate via:
        //   - the parallel paged reload above (mini thumbnails refetch from
        //     fresh `AlbumUIViewData` rows);
        //   - the user-initiated right-click "Refresh Artwork" path
        //     (`ArtworkMessage::RefreshAlbumArtwork`).

        Task::batch(tasks)
    }
}
