//! Text Input Dialog
//!
//! A reusable overlay dialog with a text input field, submit/cancel buttons,
//! and keyboard handling (Enter to submit, Escape to cancel).
//!
//! Used for Rename Playlist, Save Queue as Playlist, and similar flows
//! that need a single text input from the user.

use iced::{
    Alignment, Element, Length,
    widget::{button, checkbox, column, combo_box, container, row, text, text_input},
};

use crate::theme;

/// What action should happen when the dialog is submitted.
#[derive(Debug, Clone)]
pub enum TextInputDialogAction {
    /// Rename an existing playlist (holds playlist_id)
    RenamePlaylist(String),
    /// Create a new playlist from the current queue
    CreatePlaylistFromQueue,
    /// Overwrite an existing playlist with the current queue (holds playlist_id)
    OverwritePlaylistFromQueue(String),
    /// Delete a playlist (holds playlist_id, playlist_name)
    DeletePlaylist(String, String),
    /// Create a new playlist from specific song IDs ("Add to Playlist" flow)
    CreatePlaylistWithSongs(Vec<String>),
    /// Append song IDs to an existing playlist (playlist_id, song_ids)
    AppendToPlaylist(String, Vec<String>),
    /// Write a free-text general setting identified by key (e.g. "general.local_music_path")
    WriteGeneralSetting { key: String },
    /// Reset all non-color visualizer settings to defaults
    ResetVisualizerSettings,
    /// Reset all hotkey bindings to defaults
    ResetAllHotkeys,
    /// Create a new internet radio station
    CreateRadioStation,
    /// Delete an internet radio station (holds station id, station name)
    DeleteRadioStation(String, String),
    /// Edit an internet radio station (holds station id)
    EditRadioStation(String),
}

/// An option in the playlist selection combo_box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistOption {
    /// Create a brand new playlist
    NewPlaylist,
    /// Overwrite an existing playlist
    Existing { id: String, name: String },
}

impl std::fmt::Display for PlaylistOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewPlaylist => write!(f, "+ New playlist"),
            Self::Existing { name, .. } => write!(f, "{name}"),
        }
    }
}

/// State for the text input dialog overlay.
#[derive(Debug, Clone)]
pub struct TextInputDialogState {
    pub visible: bool,
    pub title: String,
    pub value: String,
    pub placeholder: String,
    pub action: Option<TextInputDialogAction>,
    /// combo_box state for playlist selection (only used in save-as-playlist flow)
    pub playlist_combo_state: combo_box::State<PlaylistOption>,
    /// Currently selected playlist option
    pub selected_playlist: Option<PlaylistOption>,
    /// Whether the dialog is in "save as playlist" mode (shows combo_box)
    pub save_playlist_mode: bool,
    /// Whether a newly created playlist is public. Visible in save-playlist mode
    /// when creating (not overwriting). Defaults to `true` per project policy.
    pub public: bool,
    /// Whether the dialog is in confirmation-only mode (no text input, just message + buttons)
    pub confirmation_only: bool,
    /// Warning/description message shown in confirmation-only mode
    pub confirmation_message: String,

    // Optional secondary input (for radio stations)
    pub secondary_value: Option<String>,
    pub secondary_placeholder: String,
}

impl Default for TextInputDialogState {
    fn default() -> Self {
        Self {
            visible: false,
            title: String::new(),
            value: String::new(),
            placeholder: String::new(),
            action: None,
            playlist_combo_state: combo_box::State::new(Vec::new()),
            selected_playlist: None,
            save_playlist_mode: false,
            public: true,
            confirmation_only: false,
            confirmation_message: String::new(),
            secondary_value: None,
            secondary_placeholder: String::new(),
        }
    }
}

impl TextInputDialogState {
    /// Reset all transient fields to defaults. Called at the start of every `open_*` method
    /// so each opener only needs to set the fields unique to its mode.
    fn reset_fields(&mut self) {
        self.visible = true;
        self.title.clear();
        self.value.clear();
        self.placeholder.clear();
        self.action = None;
        self.playlist_combo_state = combo_box::State::new(Vec::new());
        self.selected_playlist = None;
        self.save_playlist_mode = false;
        self.public = true;
        self.confirmation_only = false;
        self.confirmation_message.clear();
        self.secondary_value = None;
        self.secondary_placeholder.clear();
    }

    /// Open the dialog with the given configuration.
    pub fn open(
        &mut self,
        title: impl Into<String>,
        value: impl Into<String>,
        placeholder: impl Into<String>,
        action: TextInputDialogAction,
    ) {
        self.reset_fields();
        self.title = title.into();
        self.value = value.into();
        self.placeholder = placeholder.into();
        self.action = Some(action);
    }

    /// Open the dialog with two input fields.
    pub fn open_two_fields(
        &mut self,
        title: impl Into<String>,
        value1: impl Into<String>,
        placeholder1: impl Into<String>,
        value2: impl Into<String>,
        placeholder2: impl Into<String>,
        action: TextInputDialogAction,
    ) {
        self.reset_fields();
        self.title = title.into();
        self.value = value1.into();
        self.placeholder = placeholder1.into();
        self.secondary_value = Some(value2.into());
        self.secondary_placeholder = placeholder2.into();
        self.action = Some(action);
    }

    /// Open the "Save Queue as Playlist" dialog with existing playlist choices.
    ///
    /// `playlists` is a slice of `(id, name)` tuples for existing playlists.
    pub fn open_save_playlist(&mut self, playlists: &[(String, String)]) {
        self.reset_fields();
        self.title = "Save Queue as Playlist".to_string();
        self.placeholder = "Playlist name...".to_string();
        self.action = Some(TextInputDialogAction::CreatePlaylistFromQueue);

        // Build options: NewPlaylist first, then existing playlists (sorted by name)
        let mut options = Vec::with_capacity(playlists.len() + 1);
        options.push(PlaylistOption::NewPlaylist);
        for (id, name) in playlists {
            options.push(PlaylistOption::Existing {
                id: id.clone(),
                name: name.clone(),
            });
        }
        self.playlist_combo_state = combo_box::State::new(options);
        self.selected_playlist = Some(PlaylistOption::NewPlaylist);
        self.save_playlist_mode = true;
    }

    /// Open the "Add to Playlist" dialog with existing playlist choices.
    ///
    /// `playlists` is a slice of `(id, name)` tuples for existing playlists.
    /// `song_ids` are the pre-resolved song IDs to add.
    pub fn open_add_to_playlist(&mut self, playlists: &[(String, String)], song_ids: Vec<String>) {
        self.reset_fields();
        self.title = "Add to Playlist".to_string();
        self.placeholder = "Playlist name...".to_string();
        self.action = Some(TextInputDialogAction::CreatePlaylistWithSongs(song_ids));

        // Build options: NewPlaylist first, then existing playlists (sorted by name)
        let mut options = Vec::with_capacity(playlists.len() + 1);
        options.push(PlaylistOption::NewPlaylist);
        for (id, name) in playlists {
            options.push(PlaylistOption::Existing {
                id: id.clone(),
                name: name.clone(),
            });
        }
        self.playlist_combo_state = combo_box::State::new(options);
        self.selected_playlist = Some(PlaylistOption::NewPlaylist);
        self.save_playlist_mode = true;
    }

    /// Open a confirmation-only dialog (no text input, just a message + Delete/Cancel).
    pub fn open_delete_confirmation(&mut self, playlist_id: String, playlist_name: String) {
        self.reset_fields();
        self.title = "Delete Playlist".to_string();
        self.confirmation_message = format!("This will permanently delete \"{playlist_name}\".");
        self.action = Some(TextInputDialogAction::DeletePlaylist(
            playlist_id,
            playlist_name,
        ));
        self.confirmation_only = true;
    }

    /// Open a confirmation-only dialog for deleting a radio station.
    pub fn open_delete_radio_confirmation(&mut self, station_id: String, station_name: String) {
        self.reset_fields();
        self.title = "Delete Radio Station".to_string();
        self.confirmation_message = format!("This will permanently delete \"{station_name}\".");
        self.action = Some(TextInputDialogAction::DeleteRadioStation(
            station_id,
            station_name,
        ));
        self.confirmation_only = true;
    }

    /// Open a confirmation dialog for resetting visualizer settings.
    pub fn open_reset_visualizer_confirmation(&mut self) {
        self.reset_fields();
        self.title = "Reset Visualizer Settings".to_string();
        self.confirmation_message =
            "This will reset all non-color visualizer settings to their defaults.".to_string();
        self.action = Some(TextInputDialogAction::ResetVisualizerSettings);
        self.confirmation_only = true;
    }

    /// Open a confirmation dialog for resetting all hotkey bindings.
    pub fn open_reset_hotkeys_confirmation(&mut self) {
        self.reset_fields();
        self.title = "Reset All Hotkeys".to_string();
        self.confirmation_message =
            "This will restore all hotkey bindings to their defaults.".to_string();
        self.action = Some(TextInputDialogAction::ResetAllHotkeys);
        self.confirmation_only = true;
    }

    /// Close and reset the dialog.
    pub fn close(&mut self) {
        self.reset_fields();
        self.visible = false;
    }
}

/// Messages emitted by the text input dialog.
#[derive(Debug, Clone)]
pub enum TextInputDialogMessage {
    /// Text input value changed
    ValueChanged(String),
    /// Secondary text input value changed
    SecondaryValueChanged(String),
    /// User submitted (Enter key or Submit button)
    Submit,

    /// User cancelled (Escape key or Cancel button)
    Cancel,
    /// User selected a playlist option from the combo_box
    PlaylistSelected(PlaylistOption),
    /// User toggled the "Public" checkbox in save-as-playlist mode
    PublicToggled(bool),
}

/// Unique text_input ID for the dialog (for focus management)
pub(crate) const DIALOG_INPUT_ID: &str = "text_input_dialog_input";

/// Shared text_input styling for all dialog input fields (primary, secondary, combo_box).
fn dialog_input_style(_theme: &iced::Theme, status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: theme::bg0_soft().into(),
        border: iced::Border {
            color: if matches!(status, text_input::Status::Focused { .. }) {
                theme::accent_bright()
            } else {
                iced::Color::TRANSPARENT
            },
            width: 2.0,
            radius: theme::ui_border_radius(),
        },
        icon: theme::fg4(),
        placeholder: theme::fg4(),
        value: theme::fg0(),
        selection: theme::selection_color(),
    }
}

/// Render the dialog overlay. Returns `None` if not visible.
///
/// The returned Element should be pushed onto the Stack in `home_view`.
pub(crate) fn text_input_dialog_overlay<'a>(
    state: &'a TextInputDialogState,
) -> Option<Element<'a, TextInputDialogMessage>> {
    if !state.visible {
        return None;
    }

    let title_text = text(&state.title)
        .size(16)
        .font(theme::ui_font())
        .color(theme::fg0());

    let is_overwrite = matches!(
        state.selected_playlist,
        Some(PlaylistOption::Existing { .. })
    );

    // Build dialog content elements
    let mut content = column![title_text]
        .spacing(12)
        .padding(20)
        .width(Length::Fixed(360.0));

    // Playlist selection combo_box (only for save-as-playlist flow)
    if state.save_playlist_mode {
        let combo = combo_box(
            &state.playlist_combo_state,
            "Search playlists...",
            state.selected_playlist.as_ref(),
            TextInputDialogMessage::PlaylistSelected,
        )
        .width(Length::Fill)
        .size(13.0)
        .font(theme::ui_font())
        .padding([8, 8])
        .input_style(dialog_input_style)
        .menu_style(|_theme| iced::widget::overlay::menu::Style {
            text_color: theme::fg0(),
            background: theme::bg1().into(),
            border: iced::Border {
                color: theme::accent_bright(),
                width: 2.0,
                radius: theme::ui_border_radius(),
            },
            selected_text_color: theme::bg0_hard(),
            selected_background: theme::accent_bright().into(),
            shadow: iced::Shadow::default(),
        });

        // Icon label next to combo_box: list-plus for new, list-music for existing
        let icon_path = if matches!(state.selected_playlist, Some(PlaylistOption::NewPlaylist)) {
            "assets/icons/list-plus.svg"
        } else {
            "assets/icons/list-music.svg"
        };
        let combo_icon = crate::embedded_svg::svg_widget(icon_path)
            .width(Length::Fixed(16.0))
            .height(Length::Fixed(16.0))
            .style(|_theme, _status| iced::widget::svg::Style {
                color: Some(theme::accent()),
            });

        content = content.push(
            row![combo_icon, combo]
                .spacing(8)
                .align_y(Alignment::Center),
        );
    }

    // Confirmation-only mode: show warning message, no text input
    if state.confirmation_only {
        let warning = text(&state.confirmation_message)
            .size(13)
            .font(theme::ui_font())
            .color(theme::fg3());
        content = content.push(warning);
    } else if !is_overwrite {
        // Text input — shown when creating a new playlist (not overwriting) or for non-playlist flows
        let input = text_input(&state.placeholder, &state.value)
            .id(DIALOG_INPUT_ID)
            .on_input(TextInputDialogMessage::ValueChanged)
            .on_submit(TextInputDialogMessage::Submit)
            .padding(8)
            .size(14)
            .font(theme::ui_font())
            .width(Length::Fill)
            .style(dialog_input_style);
        content = content.push(input);

        if let Some(sec_val) = &state.secondary_value {
            let input2 = text_input(&state.secondary_placeholder, sec_val)
                .on_input(TextInputDialogMessage::SecondaryValueChanged)
                .on_submit(TextInputDialogMessage::Submit)
                .padding(8)
                .size(14)
                .font(theme::ui_font())
                .width(Length::Fill)
                .style(dialog_input_style);
            content = content.push(input2);
        }
    }

    // Public/Private toggle — shown when creating a new playlist via the
    // save-as-playlist flow (not overwrite, not confirmation). Default-public.
    if state.save_playlist_mode && !is_overwrite && !state.confirmation_only {
        let public_check = checkbox(state.public)
            .label("Public")
            .on_toggle(TextInputDialogMessage::PublicToggled)
            .size(14)
            .text_size(13)
            .font(theme::ui_font())
            .style(|_theme, status| {
                let is_checked = matches!(
                    status,
                    checkbox::Status::Active { is_checked: true }
                        | checkbox::Status::Hovered { is_checked: true }
                        | checkbox::Status::Disabled { is_checked: true }
                );
                checkbox::Style {
                    background: if is_checked {
                        theme::accent().into()
                    } else {
                        theme::bg0_soft().into()
                    },
                    icon_color: theme::fg0(),
                    border: iced::Border {
                        color: if is_checked {
                            theme::accent_bright()
                        } else {
                            theme::bg3()
                        },
                        width: 1.0,
                        radius: theme::ui_border_radius(),
                    },
                    text_color: Some(theme::fg2()),
                }
            });
        content = content.push(public_check);
    }

    // Overwrite/append confirmation message
    if is_overwrite && let Some(PlaylistOption::Existing { name, .. }) = &state.selected_playlist {
        let is_append = matches!(
            state.action,
            Some(TextInputDialogAction::AppendToPlaylist(_, _))
        );
        let warning_text = if is_append {
            let count = match &state.action {
                Some(TextInputDialogAction::AppendToPlaylist(_, ids)) => ids.len(),
                _ => 0,
            };
            let label = if count == 1 { "song" } else { "songs" };
            format!("Will add {count} {label} to \"{name}\".")
        } else {
            format!("This will replace all tracks in \"{name}\".")
        };
        let warning = text(warning_text)
            .size(12)
            .font(theme::ui_font())
            .color(theme::fg3());
        content = content.push(warning);
    }

    let is_destructive = state.confirmation_only
        && !matches!(
            state.action,
            Some(
                TextInputDialogAction::ResetVisualizerSettings
                    | TextInputDialogAction::ResetAllHotkeys
            )
        );

    // Submit / Cancel buttons
    let is_add_to_playlist = matches!(
        state.action,
        Some(
            TextInputDialogAction::CreatePlaylistWithSongs(_)
                | TextInputDialogAction::AppendToPlaylist(_, _)
        )
    );
    let submit_label = if state.confirmation_only {
        match &state.action {
            Some(
                TextInputDialogAction::ResetVisualizerSettings
                | TextInputDialogAction::ResetAllHotkeys,
            ) => "Reset",
            _ => "Delete",
        }
    } else if is_overwrite {
        if is_add_to_playlist {
            "Add"
        } else {
            "Overwrite"
        }
    } else if state.save_playlist_mode {
        if is_add_to_playlist { "Add" } else { "Create" }
    } else {
        "Submit"
    };

    let submit_btn = button(
        container(
            text(submit_label)
                .size(13)
                .font(theme::ui_font())
                .color(theme::fg0()),
        )
        .padding([4, 16]),
    )
    .on_press(TextInputDialogMessage::Submit)
    .style(move |_theme, status| {
        let (bg, border_color) = if is_destructive {
            // Destructive red-tinted style for delete actions
            let red = iced::Color::from_rgb(0.8, 0.2, 0.2);
            let red_bright = iced::Color::from_rgb(0.9, 0.3, 0.3);
            match status {
                button::Status::Hovered | button::Status::Pressed => (theme::bg2(), red_bright),
                _ => (theme::bg3(), red),
            }
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => {
                    (theme::bg2(), theme::accent())
                }
                _ => (theme::bg3(), theme::accent_bright()),
            }
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme::fg0(),
            border: iced::Border {
                color: border_color,
                width: 1.0,
                radius: theme::ui_border_radius(),
            },
            ..Default::default()
        }
    });

    let cancel_btn = button(
        container(
            text("Cancel")
                .size(13)
                .font(theme::ui_font())
                .color(theme::fg2()),
        )
        .padding([4, 16]),
    )
    .on_press(TextInputDialogMessage::Cancel)
    .style(|_theme, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => Some(theme::bg2().into()),
            _ => None,
        };
        button::Style {
            background: bg,
            text_color: theme::fg2(),
            border: iced::Border {
                color: theme::bg3(),
                width: 1.0,
                radius: theme::ui_border_radius(),
            },
            ..Default::default()
        }
    });

    let button_row = row![cancel_btn, submit_btn]
        .spacing(8)
        .align_y(Alignment::Center);

    content = content.push(button_row);

    // Dialog box with border
    let dialog_box = container(content)
        .style(|_theme| container::Style {
            background: Some(theme::bg1().into()),
            border: iced::Border {
                color: theme::accent_bright(),
                width: 1.0,
                radius: theme::ui_border_radius(),
            },
            ..Default::default()
        })
        .width(Length::Shrink);

    // Center the dialog in a full-screen semi-transparent backdrop
    let backdrop = container(dialog_box)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(|_theme| {
            // Semi-transparent backdrop using theme bg color (works in light mode too)
            let mut bg = theme::bg0_hard();
            bg.a = 0.6;
            container::Style {
                background: Some(bg.into()),
                ..Default::default()
            }
        });

    Some(backdrop.into())
}
