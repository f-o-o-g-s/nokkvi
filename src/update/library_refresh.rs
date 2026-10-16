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
    Nokkvi,
    app_message::Message,
    services::navidrome_sse::LibraryChange,
    update::{AlbumsTarget, ArtistsTarget, SongsTarget},
    widgets::view_header::SortMode,
};

impl Nokkvi {
    pub(crate) fn handle_library_changed(&mut self, change: LibraryChange) -> Task<Message> {
        // No session, nothing to refresh or announce. The SSE stream is torn
        // down at logout, but a star, rating or scrobble confirmed after it
        // replays its refresh here (see `star_result_message`,
        // `handle_scrobble_submission_result`), and would otherwise toast
        // "Library refreshed" on the login screen.
        if self.screen != crate::Screen::Home {
            tracing::debug!(" [SSE] LibraryChanged outside a session; nothing to refresh");
            return Task::none();
        }
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

        let refresh_libraries = change.refreshes_libraries();
        let LibraryChange {
            album_ids,
            artist_ids,
            song_ids,
            playlist_ids,
            genre_ids,
            library_ids,
            is_wildcard,
        } = change;

        info!(
            "🔄 Navidrome library changed (wildcard={is_wildcard}, albums={}, artists={}, songs={}, playlists={}, genres={}, libraries={}), initiating background refresh",
            album_ids.len(),
            artist_ids.len(),
            song_ids.len(),
            playlist_ids.len(),
            genre_ids.len(),
            library_ids.len(),
        );

        let mut tasks = Vec::new();

        // Each branch fires only when the SSE payload flagged that entity kind
        // (or signalled a wildcard / full-scan). The buffer-non-empty gate
        // skips views the user hasn't visited yet — those will fetch fresh
        // on first visit. The Random-sort gate protects the artwork
        // reference (a background reload would return a new random order and
        // jar the user mid-browse); the user can press F5 to re-randomize
        // intentionally. A paged view reloads every row down to its viewport
        // (`background_reload`), so the row it was on comes back and the
        // view stays there, or keeps its rows when scrolled too deep.
        let affects_albums = is_wildcard || !album_ids.is_empty();
        let affects_artists = is_wildcard || !artist_ids.is_empty();
        let affects_songs = is_wildcard || !song_ids.is_empty();
        let affects_playlists = is_wildcard || !playlist_ids.is_empty();
        let affects_genres = is_wildcard || !genre_ids.is_empty();

        // 1. Snapshot current viewport state and trigger reload for Albums.
        if affects_albums
            && self.albums_page.common.current_sort_mode != SortMode::Random
            && let Some(reload) = self.background_reload::<AlbumsTarget>()
        {
            tasks.push(self.handle_load_albums(Some(reload)));
        }

        // 2. Snapshot current viewport state and trigger reload for Artists.
        if affects_artists
            && self.artists_page.common.current_sort_mode != SortMode::Random
            && let Some(reload) = self.background_reload::<ArtistsTarget>()
        {
            tasks.push(self.handle_load_artists(Some(reload)));
        }

        // 3. Snapshot current viewport state and trigger reload for Songs.
        if affects_songs
            && self.songs_page.common.current_sort_mode != SortMode::Random
            && let Some(reload) = self.background_reload::<SongsTarget>()
        {
            tasks.push(self.handle_load_songs(Some(reload)));
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

        // 6. Library list refresh on a full scan and on Navidrome's `library`
        //    refresh (an admin created, renamed or deleted a library, or a
        //    user's access changed), so the popover's row count matches the
        //    server the next time it opens. `Library::Loaded` also prunes
        //    `active_library_ids` of any IDs no longer in the refreshed list.
        if refresh_libraries {
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

        // 7. Lyrics follow the library — on a WILDCARD event only. A rescan
        //    (the one thing that can change a song's lyrics on the server) is
        //    announced with no ids at all; every song-scoped `refreshResource`
        //    comes from an annotation write, which cannot touch lyrics:
        //    `core/scrobbler/play_tracker.go` on a scrobble submission,
        //    `server/subsonic/media_annotation.go` on setRating / setStar.
        //    Dropping those ids would evict the verdict for the track playing
        //    right now on every scrobble and every rating keypress, and the
        //    next kick would re-run the whole chain — including a third-party
        //    LRCLIB request — which is exactly what the negative cache exists
        //    to prevent.
        //
        //    The clear is synchronous on the UI thread (a `parking_lot` mutex,
        //    uncontended here), which is what lets the re-drive below run in
        //    the same pass: its resolve would otherwise read the stale verdict
        //    it was sent to replace. A resolve already in flight is handled by
        //    the cache's own generation guard, not by ordering.
        if is_wildcard {
            if let Some(shell) = &self.app_service {
                shell.lyrics_cache().drop_all();
            }
            // A no-op unless the current track is showing no lyrics on the
            // Queue view — so lyrics added by the rescan appear without a
            // restart, and a rendered sheet is never disturbed.
            tasks.push(self.lyrics_kick_if_unresolved());
        }

        // Notify the user gently (skipped when the user has opted to suppress
        // these notifications via Settings → General → Application).
        if !self.settings.suppress_library_refresh_toasts {
            self.toast_info("Library refreshed automatically");
        }

        // The event itself triggers no artwork refresh. Navidrome fires
        // `library_changed` on every play-count bump, so a blanket
        // auto-refresh produced a one-frame GPU-upload flicker on the large
        // artwork panel after every scrobble — see `update/albums.rs` for the
        // `Handle::from_bytes` cache-busting mechanic. Cover-art replacements
        // propagate via:
        //   - the parallel paged reload above: its post-load viewport
        //     prefetch refetches only rows whose version changed (the image
        //     hash on Navidrome 0.64+, so a play-count bump refetches
        //     nothing), and a hash change also drops the stale large art
        //     (`handle_artwork_loaded`);
        //   - the user-initiated right-click "Refresh Artwork" path
        //     (`ArtworkMessage::RefreshAlbumArtwork`).

        Task::batch(tasks)
    }
}
