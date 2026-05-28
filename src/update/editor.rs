//! Playlist-editor message handler.
//!
//! The editor operates on its OWN in-memory track buffer
//! (`Nokkvi.playlist_editor`), fully decoupled from the live play queue.
//!
//! Phase 1 only lands a no-op stub so `Message::Editor(..)` routes cleanly.
//! Real handling — populating the buffer on enter, local reorder/remove/add
//! mutations, dirty detection, and Save — lands in Phase 3+.

use iced::Task;

use crate::{
    Nokkvi,
    app_message::{EditorMessage, Message},
};

impl Nokkvi {
    /// Dispatch a [`EditorMessage`]. Phase 1 stub: no behavior yet.
    //
    // Phase 3+ replaces this body with real buffer handling.
    pub(crate) fn handle_editor_message(&mut self, _msg: EditorMessage) -> Task<Message> {
        Task::none()
    }
}
