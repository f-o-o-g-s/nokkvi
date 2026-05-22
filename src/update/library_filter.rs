//! Multi-library filter — handler stubs.
//!
//! Wave 0 coordination commit only: the real handler bodies (toggle, open
//! popover, load, refetch on toggle, deleted-library pruning) land in
//! Wave 2 Lane D. This stub exists so the central dispatcher arm in
//! `update/mod.rs` compiles before the parallel lanes diverge.

use iced::Task;

use crate::{
    Nokkvi,
    app_message::{LibraryMessage, Message},
};

impl Nokkvi {
    pub(crate) fn handle_library_message(&mut self, msg: LibraryMessage) -> Task<Message> {
        tracing::trace!(?msg, "library filter stub — Wave 2 Lane D will implement");
        Task::none()
    }
}
