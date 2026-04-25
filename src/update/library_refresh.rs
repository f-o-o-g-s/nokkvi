//! Handles Navidrome event-driven library refresh
use std::collections::HashSet;

use iced::Task;
use tracing::info;

use crate::{
    Nokkvi,
    app_message::{ArtworkMessage, Message},
};

impl Nokkvi {
    pub(crate) fn handle_library_changed(
        &mut self,
        album_ids: Vec<String>,
        is_wildcard: bool,
    ) -> Task<Message> {
        info!(
            "🔄 Navidrome library changed (wildcard={is_wildcard}, album_ids={}), initiating background refresh",
            album_ids.len()
        );

        let mut tasks = Vec::new();

        // 1. Snapshot current viewport state and trigger reload for Albums if needed
        if !self.library.albums.is_empty() {
            let offset = self.albums_page.common.slot_list.viewport_offset;
            let anchor_id = self.library.albums.get(offset).map(|a| a.id.clone());
            tasks.push(self.handle_load_albums(true, anchor_id));
        }

        // 2. Snapshot current viewport state and trigger reload for Artists
        if !self.library.artists.is_empty() {
            let offset = self.artists_page.common.slot_list.viewport_offset;
            let anchor_id = self.library.artists.get(offset).map(|a| a.id.clone());
            tasks.push(self.handle_load_artists(true, anchor_id));
        }

        // 3. Snapshot current viewport state and trigger reload for Songs
        if !self.library.songs.is_empty() {
            let offset = self.songs_page.common.slot_list.viewport_offset;
            let anchor_id = self.library.songs.get(offset).map(|a| a.id.clone());
            tasks.push(self.handle_load_songs(true, anchor_id));
        }

        // Notify the user gently
        self.toast_info("Library refreshed automatically");

        // 4. On non-wildcard events, surgically refresh artwork for the changed
        //    albums in any in-RAM Handle map. With no client-side disk cache,
        //    "refresh" here means: re-fetch from server and replace the Handle
        //    so Iced's GPU texture cache picks up the new bytes. Albums not
        //    present in any UI map will simply re-fetch on next viewport entry.
        //    Wildcards (full-library scans) skip this — we don't want a
        //    silent re-download of every cover.
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

            if refreshed_in_ui > 0 {
                let suffix = if refreshed_in_ui == 1 { "" } else { "s" };
                self.toast_info(format!(
                    "Updated artwork for {refreshed_in_ui} album{suffix}"
                ));
            }
        }

        Task::batch(tasks)
    }
}
