//! Handles Navidrome event-driven library refresh
use iced::Task;
use tracing::info;

use crate::{Nokkvi, app_message::Message};

impl Nokkvi {
    pub(crate) fn handle_library_changed(&mut self) -> Task<Message> {
        info!("🔄 Navidrome library changed event received, initiating background refresh");

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

        Task::batch(tasks)
    }
}
