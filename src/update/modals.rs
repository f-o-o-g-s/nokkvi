//! The root modal registry: which overlay modal is open, and which one is on
//! top.
//!
//! Every site that has to know about the root modals asks [`Nokkvi::top_modal`]
//! and matches on [`ActiveModal`] exhaustively, so a new modal fails to compile
//! until each site decides what it does with it: the draw order
//! (`wrap_with_global_overlays`), the key gate (`handle_raw_key_event`), Escape
//! (`handle_clear_search`), the slot-list key routing
//! (`handle_slot_list_message`) and logout (`reset_session_state`).
//!
//! Adding a modal: add the variant, give it a place in [`ActiveModal::STACK`]
//! (the order is a UX decision) and in the order pin in
//! `update/tests/modals.rs`, then follow the compile errors.

use iced::Task;

use crate::{
    Nokkvi,
    app_message::Message,
    widgets::{
        EqModalMessage, about_modal::AboutModalMessage,
        default_playlist_picker::DefaultPlaylistPickerMessage, info_modal::InfoModalMessage,
        milkdrop_picker::MilkdropPickerMessage, text_input_dialog::TextInputDialogMessage,
        trawl_modal::TrawlModalMessage,
    },
};

/// A root overlay modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveModal {
    /// Name / comment edits and confirmations.
    TextInputDialog,
    /// The 10-band equalizer.
    Eq,
    About,
    /// Get Info.
    Info,
    DefaultPlaylistPicker,
    MilkdropPicker,
    /// The Trawl mix builder.
    Trawl,
}

impl ActiveModal {
    /// Every modal, top to bottom: the draw order, and the order Escape and
    /// the keys reach them in when more than one is open.
    ///
    /// The smaller, more transient surface sits higher: the prompt above the
    /// panels, the panels above the pickers, the pickers above the Trawl
    /// workspace. So a dialog that a server reply opens late lands on top of
    /// whatever the user opened meanwhile, and a small surface opened from a
    /// big one (a prompt from a picker, Get Info from Trawl) is already above
    /// it.
    pub(crate) const STACK: [Self; 7] = [
        Self::TextInputDialog,
        Self::Eq,
        Self::About,
        Self::Info,
        Self::DefaultPlaylistPicker,
        Self::MilkdropPicker,
        Self::Trawl,
    ];
}

impl Nokkvi {
    /// Whether `modal` is open, top or not.
    pub(crate) fn modal_is_open(&self, modal: ActiveModal) -> bool {
        match modal {
            ActiveModal::TextInputDialog => self.text_input_dialog.visible,
            ActiveModal::Eq => self.eq_modal.open,
            ActiveModal::About => self.about_modal.visible,
            ActiveModal::Info => self.info_modal.visible,
            ActiveModal::DefaultPlaylistPicker => self.default_playlist_picker.is_some(),
            ActiveModal::MilkdropPicker => self.milkdrop.picker.is_some(),
            ActiveModal::Trawl => self.trawl_modal.is_some(),
        }
    }

    /// The open modal highest in [`ActiveModal::STACK`]: the one on screen,
    /// and the one that owns the keyboard.
    pub(crate) fn top_modal(&self) -> Option<ActiveModal> {
        ActiveModal::STACK
            .into_iter()
            .find(|&modal| self.modal_is_open(modal))
    }

    /// Close `modal` the way its own close button does, through its own
    /// Close message, so whatever that modal tidies on close (EQ's
    /// preset-name prompt, the MilkDrop picker's preset and lock, the Trawl
    /// save dialog's way back to the mix) happens on Escape too.
    pub(crate) fn close_modal(&mut self, modal: ActiveModal) -> Task<Message> {
        match modal {
            ActiveModal::TextInputDialog => {
                self.handle_text_input_dialog(TextInputDialogMessage::Cancel)
            }
            ActiveModal::Eq => self.handle_eq_modal(EqModalMessage::Close),
            ActiveModal::About => self.handle_about_modal(AboutModalMessage::Close),
            ActiveModal::Info => self.handle_info_modal(InfoModalMessage::Close),
            ActiveModal::DefaultPlaylistPicker => {
                self.handle_default_playlist_picker(DefaultPlaylistPickerMessage::Close)
            }
            ActiveModal::MilkdropPicker => {
                self.handle_milkdrop_picker(MilkdropPickerMessage::Close)
            }
            ActiveModal::Trawl => self.handle_trawl_modal(TrawlModalMessage::Close),
        }
    }

    /// Throw `modal` away without running its Close: logout and session
    /// expiry. Close is wrong there: the MilkDrop picker's Close reloads the
    /// preset it opened over into a renderer that is being released, and the
    /// Trawl save dialog's Cancel reopens Trawl. The modal's whole state resets,
    /// open or not, so nothing of the old session (EQ's half-typed preset
    /// name, Info's item) greets the next login.
    pub(crate) fn discard_modal(&mut self, modal: ActiveModal) {
        match modal {
            ActiveModal::TextInputDialog => {
                self.text_input_dialog =
                    crate::widgets::text_input_dialog::TextInputDialogState::default();
            }
            ActiveModal::Eq => self.eq_modal = crate::widgets::eq_modal::EqModalState::default(),
            ActiveModal::About => {
                self.about_modal = crate::widgets::about_modal::AboutModalState::default();
            }
            ActiveModal::Info => {
                self.info_modal = crate::widgets::info_modal::InfoModalState::default();
            }
            ActiveModal::DefaultPlaylistPicker => self.default_playlist_picker = None,
            ActiveModal::MilkdropPicker => self.milkdrop_picker_discard(),
            ActiveModal::Trawl => self.trawl_modal = None,
        }
    }
}
