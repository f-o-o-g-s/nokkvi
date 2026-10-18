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
use nokkvi_data::types::hotkey_config::HotkeyAction;

use crate::{
    Nokkvi,
    app_message::Message,
    widgets::{
        EqModalMessage, about_modal::AboutModalMessage,
        default_playlist_picker::DefaultPlaylistPickerMessage, info_modal::InfoModalMessage,
        milkdrop_picker::MilkdropPickerMessage, similar_artists_modal::SimilarArtistsModalMessage,
        text_input_dialog::TextInputDialogMessage, trawl_modal::TrawlModalMessage,
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
    /// Similar Artists (ListenBrainz) for a row's artist.
    SimilarArtists,
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
    /// whatever the user opened meanwhile, and a future flow that opens a
    /// small surface from a big one (say, Get Info from inside Trawl) needs
    /// no reordering.
    pub(crate) const STACK: [Self; 8] = [
        Self::TextInputDialog,
        Self::Eq,
        Self::About,
        Self::Info,
        Self::SimilarArtists,
        Self::DefaultPlaylistPicker,
        Self::MilkdropPicker,
        Self::Trawl,
    ];

    /// Whether this modal, on top, takes the key bound to `action`. Escape
    /// always passes the key gate (it closes the top modal), and the MilkDrop
    /// picker's raw arrow keys are taken before any binding is looked up;
    /// every other key is swallowed, so it never drives the view behind.
    ///
    /// The pickers take the list keys (the slot-list routing hands them over).
    /// Trawl takes them too, plus Ctrl+Enter (`ShufflePlay`, which plays the
    /// mix), `/` and [`TRAWL_TRAY_ACTIONS`]; they are admitted for Trawl only,
    /// on purpose: the other modals must keep swallowing the same keys. EQ and
    /// Get Info take the key that opened them, which closes them again.
    pub(crate) fn takes_action(self, action: HotkeyAction) -> bool {
        let list_key = matches!(
            action,
            HotkeyAction::SlotListUp | HotkeyAction::SlotListDown | HotkeyAction::Activate
        );
        match self {
            Self::MilkdropPicker | Self::DefaultPlaylistPicker => list_key,
            // Its own key closes it again, as Get Info's does.
            Self::SimilarArtists => list_key || action == HotkeyAction::SimilarArtists,
            Self::Trawl => {
                list_key
                    || matches!(
                        action,
                        HotkeyAction::ShufflePlay | HotkeyAction::FocusSearch
                    )
                    || TRAWL_TRAY_ACTIONS.contains(&action)
            }
            Self::Eq => action == HotkeyAction::ToggleEqModal,
            Self::Info => action == HotkeyAction::GetInfo,
            Self::TextInputDialog | Self::About => false,
        }
    }
}

/// The Trawl modal's keys beyond the list keys, Ctrl+Enter and `/`: the
/// tray-controls focus ring (Shift+Tab / Shift+Backspace), the value arrows
/// (the seek pair and the sort pair), Add to Queue (Shift+A, which
/// `handle_add_to_queue` routes to the mix) and Save as Playlist. Each
/// handler routes Trawl-first, so admitting them never leaks to the view
/// behind. One the search field CAPTURED already acted there (Shift+Backspace
/// deleted a character, a rebound Ctrl+V pasted), so the key gate drops it
/// rather than act twice; the IPC verbs whose twin is listed here act on the
/// mix too (`key_gate_instead`).
pub(crate) const TRAWL_TRAY_ACTIONS: &[HotkeyAction] = &[
    HotkeyAction::SettingsCategoryNext,
    HotkeyAction::SettingsCategoryPrev,
    HotkeyAction::PrevSortMode,
    HotkeyAction::NextSortMode,
    HotkeyAction::SeekBackward,
    HotkeyAction::SeekForward,
    HotkeyAction::AddToQueue,
    HotkeyAction::TrawlSaveAsPlaylist,
];

impl Nokkvi {
    /// Whether `modal` is open, top or not.
    pub(crate) fn modal_is_open(&self, modal: ActiveModal) -> bool {
        match modal {
            ActiveModal::TextInputDialog => self.text_input_dialog.visible,
            ActiveModal::Eq => self.eq_modal.open,
            ActiveModal::About => self.about_modal.visible,
            ActiveModal::Info => self.info_modal.visible,
            ActiveModal::SimilarArtists => self.similar_artists_modal.is_some(),
            ActiveModal::DefaultPlaylistPicker => self.default_playlist_picker.is_some(),
            ActiveModal::MilkdropPicker => self.milkdrop.picker.is_some(),
            ActiveModal::Trawl => self.trawl.modal.is_some(),
        }
    }

    /// The open modal highest in [`ActiveModal::STACK`]: the one on screen,
    /// and the one that owns the keyboard.
    pub(crate) fn top_modal(&self) -> Option<ActiveModal> {
        ActiveModal::STACK
            .into_iter()
            .find(|&modal| self.modal_is_open(modal))
    }

    /// Whether `modal` is open AND on top. A modal's own key routes ask this,
    /// not [`Self::modal_is_open`]: one covered by another modal takes none.
    pub(crate) fn modal_is_top(&self, modal: ActiveModal) -> bool {
        self.top_modal() == Some(modal)
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
            ActiveModal::SimilarArtists => {
                self.handle_similar_artists_modal(SimilarArtistsModalMessage::Close)
            }
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
            ActiveModal::SimilarArtists => self.similar_artists_modal = None,
            ActiveModal::DefaultPlaylistPicker => self.default_playlist_picker = None,
            ActiveModal::MilkdropPicker => self.milkdrop_picker_discard(),
            ActiveModal::Trawl => self.trawl.modal = None,
        }
    }
}
