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
        keyboard::Key::Unidentified => None,
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
        HotkeyAction::SwitchToRadios => Message::SwitchView(View::Radios),
        HotkeyAction::SwitchToSettings => Message::ToggleSettings,
        // Playback
        HotkeyAction::TogglePlay => Message::Playback(PlaybackMessage::TogglePlay),
        HotkeyAction::ToggleRandom => Message::Playback(PlaybackMessage::ToggleRandom),
        HotkeyAction::ToggleRepeat => Message::Playback(PlaybackMessage::ToggleRepeat),
        HotkeyAction::ToggleConsume => Message::Playback(PlaybackMessage::ToggleConsume),
        HotkeyAction::ToggleSoundEffects => Message::Playback(PlaybackMessage::ToggleSoundEffects),
        HotkeyAction::CycleVisualization => Message::Playback(PlaybackMessage::CycleVisualization),
        HotkeyAction::ToggleEqModal => Message::EqModal(crate::widgets::EqModalMessage::Toggle),
        HotkeyAction::ToggleCrossfade => Message::Playback(PlaybackMessage::ToggleCrossfade),
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
        HotkeyAction::FindSimilar => Message::Hotkey(HotkeyMessage::FindSimilar),
        HotkeyAction::FindTopSongs => Message::Hotkey(HotkeyMessage::FindTopSongs),
        // Queue reorder
        HotkeyAction::MoveTrackUp => Message::Hotkey(HotkeyMessage::MoveTrackUp),
        HotkeyAction::MoveTrackDown => Message::Hotkey(HotkeyMessage::MoveTrackDown),
        // Queue actions
        HotkeyAction::SaveQueueAsPlaylist => Message::Hotkey(HotkeyMessage::SaveQueueAsPlaylist),
        // Sort & view
        HotkeyAction::PrevSortMode => Message::Hotkey(HotkeyMessage::CycleSortMode(false)),
        HotkeyAction::NextSortMode => Message::Hotkey(HotkeyMessage::CycleSortMode(true)),
        HotkeyAction::ToggleSortOrder => Message::SlotList(SlotListMessage::ToggleSortOrder),
        HotkeyAction::RefreshView => Message::Hotkey(HotkeyMessage::RefreshView),
        HotkeyAction::Roulette => Message::Hotkey(HotkeyMessage::StartRoulette),
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

#[cfg(test)]
mod tests {
    use nokkvi_data::types::hotkey_config::KeyCombo;

    use super::*;

    #[test]
    fn test_refresh_view_hotkey() {
        let config = HotkeyConfig::default();
        let action = config
            .lookup(&KeyCode::Char('r'), false, false, false)
            .unwrap();
        assert_eq!(action, HotkeyAction::RefreshView);

        let msg = action_to_message(action);
        // We verify that it dispatches the correct Message variant
        assert!(matches!(msg, Message::Hotkey(HotkeyMessage::RefreshView)));
    }

    // ====================================================================
    // iced_key_to_keycode — shifted character normalization
    // ====================================================================

    #[test]
    fn shifted_symbols_normalized_to_base_key() {
        // US layout: Shift+4 produces '$', but we need to map it back to '4'
        // so that bindings stored as "Shift + 4" match the iced key event.
        let cases = vec![
            ('!', '1'),
            ('@', '2'),
            ('#', '3'),
            ('$', '4'),
            ('%', '5'),
            ('^', '6'),
            ('&', '7'),
            ('*', '8'),
            ('(', '9'),
            (')', '0'),
            ('_', '-'),
            ('+', '='),
            ('~', '`'),
            ('{', '['),
            ('}', ']'),
            ('|', '\\'),
            (':', ';'),
            ('"', '\''),
            ('<', ','),
            ('>', '.'),
            ('?', '/'),
        ];

        for (shifted, base) in cases {
            let key = keyboard::Key::Character(shifted.to_string().into());
            let result = iced_key_to_keycode(&key);
            assert_eq!(
                result,
                Some(KeyCode::Char(base)),
                "Shifted char '{shifted}' should normalize to base '{base}'"
            );
        }
    }

    #[test]
    fn uppercase_letters_normalized_to_lowercase() {
        // 'A' with shift held → should still produce KeyCode::Char('a')
        let key = keyboard::Key::Character("A".into());
        let result = iced_key_to_keycode(&key);
        assert_eq!(result, Some(KeyCode::Char('a')));

        let key = keyboard::Key::Character("Z".into());
        let result = iced_key_to_keycode(&key);
        assert_eq!(result, Some(KeyCode::Char('z')));
    }

    #[test]
    fn lowercase_letters_pass_through() {
        let key = keyboard::Key::Character("a".into());
        let result = iced_key_to_keycode(&key);
        assert_eq!(result, Some(KeyCode::Char('a')));
    }

    #[test]
    fn special_chars_pass_through() {
        // Characters that are NOT shifted symbols pass through as-is (lowercased)
        let key = keyboard::Key::Character("/".into());
        assert_eq!(iced_key_to_keycode(&key), Some(KeyCode::Char('/')));

        let key = keyboard::Key::Character("-".into());
        assert_eq!(iced_key_to_keycode(&key), Some(KeyCode::Char('-')));

        let key = keyboard::Key::Character("`".into());
        assert_eq!(iced_key_to_keycode(&key), Some(KeyCode::Char('`')));
    }

    // ====================================================================
    // iced_key_to_keycode — named key mapping
    // ====================================================================

    #[test]
    fn named_keys_mapped_correctly() {
        use keyboard::key;

        let cases = vec![
            (key::Named::Space, KeyCode::Space),
            (key::Named::Enter, KeyCode::Enter),
            (key::Named::Escape, KeyCode::Escape),
            (key::Named::Backspace, KeyCode::Backspace),
            (key::Named::Tab, KeyCode::Tab),
            (key::Named::ArrowUp, KeyCode::ArrowUp),
            (key::Named::ArrowDown, KeyCode::ArrowDown),
            (key::Named::ArrowLeft, KeyCode::ArrowLeft),
            (key::Named::ArrowRight, KeyCode::ArrowRight),
            (key::Named::PageUp, KeyCode::PageUp),
            (key::Named::PageDown, KeyCode::PageDown),
            (key::Named::Home, KeyCode::Home),
            (key::Named::End, KeyCode::End),
            (key::Named::Delete, KeyCode::Delete),
            (key::Named::Insert, KeyCode::Insert),
            (key::Named::F1, KeyCode::F1),
            (key::Named::F12, KeyCode::F12),
        ];

        for (named, expected) in cases {
            let key = keyboard::Key::Named(named);
            assert_eq!(
                iced_key_to_keycode(&key),
                Some(expected.clone()),
                "Named key {named:?} should map to {expected:?}"
            );
        }
    }

    #[test]
    fn unrecognized_named_key_returns_none() {
        // Keys we don't bind to (e.g., CapsLock, PrintScreen) return None
        use keyboard::key;
        let key = keyboard::Key::Named(key::Named::CapsLock);
        assert_eq!(iced_key_to_keycode(&key), None);
    }

    #[test]
    fn empty_character_returns_none() {
        // Edge case: empty character string
        let key = keyboard::Key::Character("".into());
        assert_eq!(iced_key_to_keycode(&key), None);
    }

    // ====================================================================
    // handle_hotkey — end-to-end dispatch
    // ====================================================================

    #[test]
    fn handle_hotkey_dispatches_navigation() {
        let config = HotkeyConfig::default();
        let modifiers = keyboard::Modifiers::default();

        // Key '1' (no modifiers) → SwitchView(Queue)
        let key = keyboard::Key::Character("1".into());
        let msg = handle_hotkey(key, modifiers, &config);
        assert!(
            matches!(msg, Some(Message::SwitchView(View::Queue))),
            "Key '1' should switch to Queue view"
        );

        // Key '2' → Albums
        let key = keyboard::Key::Character("2".into());
        let msg = handle_hotkey(key, modifiers, &config);
        assert!(matches!(msg, Some(Message::SwitchView(View::Albums))));
    }

    #[test]
    fn handle_hotkey_dispatches_playback() {
        let config = HotkeyConfig::default();
        let modifiers = keyboard::Modifiers::default();

        // Space → TogglePlay
        let key = keyboard::Key::Named(keyboard::key::Named::Space);
        let msg = handle_hotkey(key, modifiers, &config);
        assert!(matches!(
            msg,
            Some(Message::Playback(PlaybackMessage::TogglePlay))
        ));
    }

    #[test]
    fn handle_hotkey_with_shift_modifier() {
        let config = HotkeyConfig::default();
        let modifiers = keyboard::Modifiers::SHIFT;

        // Shift + 'l' → ToggleStar (via HotkeyMessage)
        let key = keyboard::Key::Character("l".into());
        let msg = handle_hotkey(key, modifiers, &config);
        assert!(
            matches!(msg, Some(Message::Hotkey(HotkeyMessage::ToggleStar))),
            "Shift+L should dispatch ToggleStar"
        );
    }

    #[test]
    fn handle_hotkey_shifted_symbol_resolved() {
        let config = HotkeyConfig::default();
        let modifiers = keyboard::Modifiers::SHIFT;

        // When user presses Shift+4 on US keyboard, iced may report char '$'
        // with shift modifier. Our normalization maps '$' → '4', so this should
        // successfully match the SwitchToSongs binding (key '4').
        // NOTE: However, shift is now TRUE, so the combo is "Shift + 4", which
        // may not match "4" (no shift). This is a known edge case — view switch
        // keys use bare key combos without modifiers.
        let key = keyboard::Key::Character("$".into());
        let msg = handle_hotkey(key, modifiers, &config);
        // '$' normalizes to '4', but combo is (Char('4'), shift=true, ctrl=false, alt=false)
        // Default binding for SwitchToSongs is (Char('4'), shift=false) — so no match.
        // This is expected behavior: shifted symbols don't accidentally trigger
        // number-key navigation.
        assert!(
            msg.is_none(),
            "Shift+$ should not match '4' (SwitchToSongs) because shift is held"
        );
    }

    #[test]
    fn handle_hotkey_unbound_key_returns_none() {
        let config = HotkeyConfig::default();
        let modifiers = keyboard::Modifiers::default();

        // F12 is not bound to anything by default
        let key = keyboard::Key::Named(keyboard::key::Named::F12);
        let msg = handle_hotkey(key, modifiers, &config);
        assert!(msg.is_none(), "F12 should not be bound by default");
    }

    #[test]
    fn handle_hotkey_custom_binding_works() {
        let mut config = HotkeyConfig::default();
        // Rebind ToggleStar from Shift+L to F5
        config.set_binding(HotkeyAction::ToggleStar, KeyCombo::key(KeyCode::F5));

        let modifiers = keyboard::Modifiers::default();
        let key = keyboard::Key::Named(keyboard::key::Named::F5);
        let msg = handle_hotkey(key, modifiers, &config);
        assert!(
            matches!(msg, Some(Message::Hotkey(HotkeyMessage::ToggleStar))),
            "Custom F5 binding should dispatch ToggleStar"
        );
    }
}
