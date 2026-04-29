//! Open-menu state handler.
//!
//! Single source of truth for which overlay menu is open across the app
//! (hamburger, player-bar kebab, view-header checkbox dropdown, right-click
//! context menu). Replacing the value implicitly closes any previously open
//! menu, which is what enforces mutual exclusion.

use iced::Task;

use crate::{
    Nokkvi,
    app_message::{Message, OpenMenu},
};

impl Nokkvi {
    /// Set (or clear) the currently open overlay menu.
    ///
    /// Pass `Some(open_menu)` to open a menu — any previously open menu is
    /// closed automatically. Pass `None` to close whatever is open.
    pub fn handle_set_open_menu(&mut self, next: Option<OpenMenu>) -> Task<Message> {
        self.open_menu = next;
        Task::none()
    }
}
