//! Global Hotkey Handlers
//!
//! Application-wide keyboard shortcuts that work regardless of current view.
//! Uses `HotkeyConfig` for configurable bindings instead of hardcoded match arms.

use iced::keyboard;
use nokkvi_data::types::hotkey_config::{HotkeyAction, HotkeyConfig, KeyCode};

use crate::{
    Message, View,
    app_message::{HotkeyMessage, PlaybackMessage, SlotListMessage},
};

/// Convert an iced keyboard key to our `KeyCode` enum.
/// Returns `None` for keys we don't support binding to.
///
/// Normalises shifted characters back to their base key (e.g. `$` → `4`)
/// so that bindings stored as `Shift + 4` match the actual key event
/// that Iced reports as `Key::Character("$")` with `modifiers.shift()`.
pub(crate) fn iced_key_to_keycode(key: &keyboard::Key) -> Option<KeyCode> {
    use keyboard::key;
    match key {
        keyboard::Key::Character(c) => {
            let ch = c.chars().next()?;
            // Map shifted symbols back to their unshifted base key so that
            // bindings like `Shift + 4` work regardless of whether iced
            // reports the character as '4' or '$'.
            let base = match ch {
                '!' => '1',
                '@' => '2',
                '#' => '3',
                '$' => '4',
                '%' => '5',
                '^' => '6',
                '&' => '7',
                '*' => '8',
                '(' => '9',
                ')' => '0',
                '_' => '-',
                '+' => '=',
                '~' => '`',
                '{' => '[',
                '}' => ']',
                '|' => '\\',
                ':' => ';',
                '"' => '\'',
                '<' => ',',
                '>' => '.',
                '?' => '/',
                other => other.to_lowercase().next().unwrap_or(other),
            };
            Some(KeyCode::Char(base))
        }
        keyboard::Key::Named(named) => match named {
            key::Named::Space => Some(KeyCode::Space),
            key::Named::Enter => Some(KeyCode::Enter),
            key::Named::Escape => Some(KeyCode::Escape),
            key::Named::Backspace => Some(KeyCode::Backspace),
            key::Named::Tab => Some(KeyCode::Tab),
            key::Named::ArrowUp => Some(KeyCode::ArrowUp),
            key::Named::ArrowDown => Some(KeyCode::ArrowDown),
            key::Named::ArrowLeft => Some(KeyCode::ArrowLeft),
            key::Named::ArrowRight => Some(KeyCode::ArrowRight),
            key::Named::PageUp => Some(KeyCode::PageUp),
            key::Named::PageDown => Some(KeyCode::PageDown),
            key::Named::Home => Some(KeyCode::Home),
            key::Named::End => Some(KeyCode::End),
            key::Named::Delete => Some(KeyCode::Delete),
            key::Named::Insert => Some(KeyCode::Insert),
            key::Named::F1 => Some(KeyCode::F1),
            key::Named::F2 => Some(KeyCode::F2),
            key::Named::F3 => Some(KeyCode::F3),
            key::Named::F4 => Some(KeyCode::F4),
            key::Named::F5 => Some(KeyCode::F5),
            key::Named::F6 => Some(KeyCode::F6),
            key::Named::F7 => Some(KeyCode::F7),
            key::Named::F8 => Some(KeyCode::F8),
            key::Named::F9 => Some(KeyCode::F9),
            key::Named::F10 => Some(KeyCode::F10),
            key::Named::F11 => Some(KeyCode::F11),
            key::Named::F12 => Some(KeyCode::F12),
            _ => None,
        },
        _ => None,
    }
}

/// Convert a `HotkeyAction` to the corresponding `Message`.
fn action_to_message(action: HotkeyAction) -> Message {
    match action {
        // Navigation
        HotkeyAction::SwitchToQueue => Message::SwitchView(View::Queue),
        HotkeyAction::SwitchToAlbums => Message::SwitchView(View::Albums),
        HotkeyAction::SwitchToArtists => Message::SwitchView(View::Artists),
        HotkeyAction::SwitchToSongs => Message::SwitchView(View::Songs),
        HotkeyAction::SwitchToGenres => Message::SwitchView(View::Genres),
        HotkeyAction::SwitchToPlaylists => Message::SwitchView(View::Playlists),
        HotkeyAction::SwitchToSettings => Message::ToggleSettings,
        // Playback
        HotkeyAction::TogglePlay => Message::Playback(PlaybackMessage::TogglePlay),
        HotkeyAction::ToggleRandom => Message::Playback(PlaybackMessage::ToggleRandom),
        HotkeyAction::ToggleRepeat => Message::Playback(PlaybackMessage::ToggleRepeat),
        HotkeyAction::ToggleConsume => Message::Playback(PlaybackMessage::ToggleConsume),
        HotkeyAction::ToggleSoundEffects => Message::Playback(PlaybackMessage::ToggleSoundEffects),
        HotkeyAction::CycleVisualization => Message::Playback(PlaybackMessage::CycleVisualization),
        // Slot List
        HotkeyAction::SlotListUp => Message::SlotList(SlotListMessage::NavigateUp),
        HotkeyAction::SlotListDown => Message::SlotList(SlotListMessage::NavigateDown),
        HotkeyAction::Activate => Message::SlotList(SlotListMessage::ActivateCenter),
        HotkeyAction::ExpandCenter => Message::Hotkey(HotkeyMessage::ExpandCenter),
        // Browse
        HotkeyAction::ToggleBrowsingPanel => Message::ToggleBrowsingPanel,
        HotkeyAction::CenterOnPlaying => Message::Hotkey(HotkeyMessage::CenterOnPlaying),
        HotkeyAction::ToggleStar => Message::Hotkey(HotkeyMessage::ToggleStar),
        HotkeyAction::AddToQueue => Message::Hotkey(HotkeyMessage::AddToQueue),
        HotkeyAction::RemoveFromQueue => Message::Hotkey(HotkeyMessage::RemoveFromQueue),
        HotkeyAction::ClearQueue => Message::Hotkey(HotkeyMessage::ClearQueue),
        HotkeyAction::FocusSearch => Message::Hotkey(HotkeyMessage::FocusSearch),
        HotkeyAction::IncreaseRating => Message::Hotkey(HotkeyMessage::IncreaseRating),
        HotkeyAction::DecreaseRating => Message::Hotkey(HotkeyMessage::DecreaseRating),
        HotkeyAction::GetInfo => Message::Hotkey(HotkeyMessage::GetInfo),
        // Queue reorder
        HotkeyAction::MoveTrackUp => Message::Hotkey(HotkeyMessage::MoveTrackUp),
        HotkeyAction::MoveTrackDown => Message::Hotkey(HotkeyMessage::MoveTrackDown),
        // Queue actions
        HotkeyAction::SaveQueueAsPlaylist => Message::Hotkey(HotkeyMessage::SaveQueueAsPlaylist),
        // Sort & view
        HotkeyAction::PrevSortMode => Message::Hotkey(HotkeyMessage::CycleSortMode(false)),
        HotkeyAction::NextSortMode => Message::Hotkey(HotkeyMessage::CycleSortMode(true)),
        HotkeyAction::ToggleSortOrder => Message::SlotList(SlotListMessage::ToggleSortOrder),
        // Settings edit
        HotkeyAction::EditUp => Message::Hotkey(HotkeyMessage::EditValue(true)),
        HotkeyAction::EditDown => Message::Hotkey(HotkeyMessage::EditValue(false)),
        // Global
        HotkeyAction::Escape => Message::Hotkey(HotkeyMessage::ClearSearch),
        HotkeyAction::ResetToDefault => {
            Message::Settings(crate::views::SettingsMessage::ResetToDefault)
        }
    }
}

/// Handle keyboard shortcuts at the application level.
///
/// Converts iced key events to `KeyCode`, looks up the bound action
/// in the `HotkeyConfig`, and returns the corresponding `Message`.
pub(crate) fn handle_hotkey(
    key: keyboard::Key,
    modifiers: keyboard::Modifiers,
    config: &HotkeyConfig,
) -> Option<Message> {
    let keycode = iced_key_to_keycode(&key)?;
    let action = config.lookup(
        &keycode,
        modifiers.shift(),
        modifiers.control(),
        modifiers.alt(),
    )?;
    Some(action_to_message(action))
}
